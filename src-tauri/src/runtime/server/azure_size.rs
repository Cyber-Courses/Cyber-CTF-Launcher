//! Azure: can this subscription create the lab VM size in the account's region? New
//! subscriptions often can't: a size isn't offered in the region, it is offered but not to this
//! subscription (student and free ones are refused most sizes almost everywhere), or its family
//! has no core quota left. All seen on real runs; each makes a launch fail late.
//!
//! `az vm list-skus` answers the first two, and is slow (a minute is common): Azure filters its
//! list by location only, so the region's list is read once, every size of it, which also names
//! the sizes this subscription can use there; the size's list across every region is read only
//! when it can't be had in this one, to name where it can. Both are kept for the session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use super::checks::Check;
use super::profile::HostProfile;
use crate::error::Result;
use crate::platform::exec::run_cli;

/// The size labs use when the account sets none: Isoloom's default for a small lab.
const DEFAULT_SIZE: &str = "Standard_D2als_v6";

/// How many regions or sizes a failed check names, at most.
const SUGGESTED: usize = 3;

/// A VM size in one location, as `az vm list-skus --all` lists it.
#[derive(Clone, Debug, PartialEq)]
struct Sku {
    name: String,
    /// Its quota family (lowercase), as `az vm list-usage` names it.
    family: String,
    /// The location this entry is about (lowercase).
    location: String,
    /// Why this subscription can't create it in `location` (Azure's reason code), if it can't.
    restricted: Option<String>,
    vcpus: Option<u32>,
    memory_gb: Option<f64>,
}

impl Sku {
    fn is(&self, size: &str) -> bool {
        self.name.eq_ignore_ascii_case(size)
    }
}

/// The sizes in `az vm list-skus --all -o json`'s answer: each with the location it is about and
/// whether a Location restriction keeps this subscription from it there (a Zone restriction
/// only closes some zones; a lab's VM takes none). Lenient with missing fields; an answer that
/// isn't a list is an error.
fn skus(json: &str) -> std::result::Result<Vec<Sku>, String> {
    let list: Vec<Value> = serde_json::from_str(json).map_err(|e| format!("its answer isn't the list of sizes ({e})"))?;
    Ok(list.iter().filter_map(sku).collect())
}

fn sku(v: &Value) -> Option<Sku> {
    let lower = |v: &Value| v.as_str().map(str::to_lowercase);
    let location = v["locations"].get(0).and_then(lower).or_else(|| v["locationInfo"].get(0).and_then(|l| lower(&l["location"]))).unwrap_or_default();
    let restricted = v["restrictions"].as_array().into_iter().flatten().find_map(|r| {
        let places = r["restrictionInfo"]["locations"].as_array().or_else(|| r["values"].as_array());
        let here = places.is_none_or(|p| p.iter().any(|l| lower(l).as_deref() == Some(location.as_str())));
        (r["type"].as_str() == Some("Location") && here).then(|| r["reasonCode"].as_str().unwrap_or("restricted").to_string())
    });
    let capability = |name: &str| v["capabilities"].as_array()?.iter().find(|c| c["name"].as_str() == Some(name))?["value"].as_str();
    Some(Sku {
        name: v["name"].as_str()?.to_string(),
        family: v["family"].as_str().unwrap_or_default().to_lowercase(),
        location,
        restricted,
        vcpus: capability("vCPUs").and_then(|n| n.parse().ok()),
        memory_gb: capability("MemoryGB").and_then(|n| n.parse().ok()),
    })
}

/// Where the size stands in the region: its quota family when this subscription may create it,
/// else why not.
#[derive(Debug, PartialEq)]
enum InRegion {
    Usable(String),
    NotOffered,
    /// Offered there, but not to this subscription: Azure's reason code.
    Restricted(String),
}

fn in_region(region_skus: &[Sku], size: &str) -> InRegion {
    match region_skus.iter().find(|s| s.is(size)) {
        None => InRegion::NotOffered,
        Some(Sku { restricted: Some(why), .. }) => InRegion::Restricted(why.clone()),
        Some(s) => InRegion::Usable(s.family.clone()),
    }
}

/// What a size that can't be had in `region` comes with: other regions that offer it to this
/// subscription (never `region`), or, when it is refused to the subscription almost everywhere
/// (in four of five regions offering it, or more), sizes like it that it can use in `region`.
/// `everywhere` is the size across every region, None when it couldn't be read.
fn suggestion(size: &str, region: &str, everywhere: Option<&[Sku]>, region_skus: &[Sku]) -> String {
    let alike: Vec<&str> = alike_sizes(size, everywhere, region_skus).iter().map(|s| s.name.as_str()).collect();
    let sizes = |lead: &str| if alike.is_empty() { lead.to_string() } else { format!("{lead} Sizes it can use in {region}: {}.", alike.join(", ")) };
    let Some(everywhere) = everywhere else {
        return if alike.is_empty() { String::new() } else { format!(" Sizes this subscription can use in {region}: {}.", alike.join(", ")) };
    };
    let mut regions: Vec<&str> =
        everywhere.iter().filter(|s| s.is(size) && s.restricted.is_none() && s.location != region).map(|s| s.location.as_str()).collect();
    regions.sort_unstable();
    regions.dedup();
    let refused = everywhere.iter().filter(|s| s.is(size) && s.restricted.is_some()).count();
    if !regions.is_empty() && refused * 5 < (refused + regions.len()) * 4 {
        regions.truncate(SUGGESTED);
        return format!(" This subscription can use it in {}.", listed(&regions));
    }
    if refused > 0 {
        return sizes(" This subscription is refused it in almost every region, as student and free subscriptions often are.");
    }
    sizes(" No other region offers it to this subscription.")
}

/// Sizes this subscription can use in the region that are like `size`: as many vCPUs (else the
/// fewest more), at least its memory, the smallest first.
fn alike_sizes<'a>(size: &str, everywhere: Option<&[Sku]>, region_skus: &'a [Sku]) -> Vec<&'a Sku> {
    let model = region_skus.iter().chain(everywhere.into_iter().flatten()).find(|s| s.is(size));
    let (vcpus, memory) = (model.and_then(|m| m.vcpus).unwrap_or(2), model.and_then(|m| m.memory_gb).unwrap_or(0.0));
    let mut usable: Vec<&Sku> = region_skus
        .iter()
        .filter(|s| !s.is(size) && s.restricted.is_none() && s.vcpus.is_some_and(|n| n >= vcpus) && s.memory_gb.unwrap_or(0.0) >= memory)
        .collect();
    usable.sort_by(|a, b| {
        (a.vcpus, a.memory_gb.unwrap_or(0.0))
            .partial_cmp(&(b.vcpus, b.memory_gb.unwrap_or(0.0)))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    usable.truncate(SUGGESTED);
    usable
}

/// "a", "a or b", "a, b or c".
fn listed(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// Free cores in a VM family's quota, from `az vm list-usage` (numbers come as strings or ints).
fn free_cores(usage: &[Value], family: &str) -> Option<i64> {
    usage.iter().find(|x| x["name"]["value"].as_str().map(str::to_lowercase).as_deref() == Some(family)).map(|x| {
        let n = |v: &Value| v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64()).unwrap_or(0);
        n(&x["limit"]) - n(&x["currentValue"])
    })
}

/// The VM size check from the family's quota in `az vm list-usage`' answer; one that can't be
/// had says why.
fn quota_check(usage_json: &Result<String>, family: &str, size: &str, region: &str) -> Check {
    let usage = match usage_json {
        Ok(out) => serde_json::from_str::<Vec<Value>>(out).map_err(|e| format!("its answer isn't the list of quotas ({e})")),
        Err(e) => Err(e.to_string()),
    };
    let usage = match usage {
        Ok(usage) => usage,
        Err(why) => return Check::warn("VM size", format!("Couldn't read the core quota for {size} in {region} ({why}); a launch may still fail on it.")),
    };
    match free_cores(&usage, family) {
        Some(f) if f >= 2 => Check::ok("VM size", format!("{size} is available in {region} ({f} cores of quota free).")),
        Some(_) => {
            Check::fail("VM size", format!("No core quota left for {size} in {region}. Request more in the Azure portal (Quotas), or try another region."))
        }
        None => Check::warn("VM size", format!("Couldn't read the core quota for {size} in {region}; a launch may still fail on it.")),
    }
}

/// The sizes lists read this session, by subscription and what was asked: Azure's answer
/// changes when a subscription is granted a size, rarely, and a minute per read is felt.
static READ: Mutex<Option<HashMap<String, Arc<Vec<Sku>>>>> = Mutex::new(None);

/// `az vm list-skus --all` for `subscription`, of `region` or (None) of every region, narrowed to
/// `size` when given; kept for the session once read. An `az` that fails (signed out, throttled,
/// no network) or answers something else is an error, never an empty list.
async fn list_skus(subscription: &str, region: Option<&str>, size: Option<&str>) -> std::result::Result<Arc<Vec<Sku>>, String> {
    let key = format!("{subscription}|{}|{}", region.unwrap_or("*"), size.unwrap_or("*"));
    if let Some(hit) = READ.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_default().get(&key) {
        return Ok(hit.clone());
    }
    let mut args = vec!["vm", "list-skus", "--subscription", subscription, "--resource-type", "virtualMachines", "--all", "-o", "json"];
    if let Some(region) = region {
        args.extend(["-l", region]);
    }
    if let Some(size) = size {
        args.extend(["--size", size]);
    }
    let read = Arc::new(skus(&run_cli("az", &args).await.map_err(|e| e.to_string())?)?);
    READ.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_default().insert(key, read.clone());
    Ok(read)
}

/// Azure: whether this subscription can create the lab VM size in the account's region, with its
/// quota; when it can't, why (not offered there, or not to this subscription) and what to try.
pub(super) async fn capacity_check(h: &HostProfile) -> Check {
    // The size labs use: the one set on the account, else Isoloom's default for a small lab.
    let size = h.size().unwrap_or(DEFAULT_SIZE);
    let (region, subscription) = (h.region().unwrap_or_default().to_lowercase(), h.cloud_account().unwrap_or_default());
    let usage_args = ["vm", "list-usage", "--subscription", subscription, "-l", region.as_str(), "-o", "json"];
    let (region_skus, usage) = tokio::join!(list_skus(subscription, Some(&region), None), run_cli("az", &usage_args));
    let region_skus = match region_skus {
        Ok(skus) => skus,
        Err(why) => return Check::warn("VM size", format!("Couldn't ask Azure whether {size} is offered in {region}: {why}")),
    };
    let fault = match in_region(&region_skus, size) {
        InRegion::Usable(family) => return quota_check(&usage, &family, size, &region),
        InRegion::NotOffered => format!("{size} isn't offered in {region}."),
        InRegion::Restricted(why) if why == "QuotaId" => format!("{size} is offered in {region}, but not to this kind of subscription (Azure: QuotaId)."),
        InRegion::Restricted(why) => format!("{size} is offered in {region}, but not to this subscription (Azure: {why})."),
    };
    let everywhere = list_skus(subscription, None, Some(size)).await.ok();
    Check::fail("VM size", fault + &suggestion(size, &region, everywhere.as_deref().map(Vec::as_slice), &region_skus))
}

#[cfg(test)]
mod tests;
