//! Azure: can this subscription create the lab VM size in the account's region? New
//! subscriptions often can't: a size isn't offered in the region, it is offered but not to this
//! subscription (student and free ones are refused most sizes almost everywhere), or its family
//! has no core quota left. All seen on real runs; each makes a launch fail late.
//!
//! isothrum-machine asks Azure ([`azure::sizes`], [`azure::quotas`]), which is slow (a minute is
//! common): the region's list is read once, every size of it, which also names the sizes this
//! subscription can use there; the size's list across every region is read only when it can't be
//! had in this one, to name where it can. Both are kept for the session. The launcher says what
//! the lists mean.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use isothrum_machine::Failure;
use isothrum_machine::cloud::azure::{self, InRegion, Size};

use super::checks::Check;
use super::profile::HostProfile;
use crate::cloud_cli::machine;

/// The size labs use when the account sets none: Isoloom's default for a small lab.
const DEFAULT_SIZE: &str = "Standard_D2als_v6";

/// How many regions or sizes a failed check names, at most.
const SUGGESTED: usize = 3;

/// What a size that can't be had in `region` comes with: other regions that offer it to this
/// subscription (never `region`), or, when it is refused to the subscription almost everywhere
/// (in four of five regions offering it, or more), sizes like it that it can use in `region`.
/// `everywhere` is the size across every region, None when it couldn't be read.
fn suggestion(size: &str, region: &str, everywhere: Option<&[Size]>, region_sizes: &[Size]) -> String {
    let alike: Vec<&str> = azure::alike(size, everywhere, region_sizes, SUGGESTED).iter().map(|s| s.name.as_str()).collect();
    let sizes = |lead: &str| if alike.is_empty() { lead.to_string() } else { format!("{lead} Sizes it can use in {region}: {}.", alike.join(", ")) };
    let Some(everywhere) = everywhere else {
        return if alike.is_empty() { String::new() } else { format!(" Sizes this subscription can use in {region}: {}.", alike.join(", ")) };
    };
    let (mut regions, refused) = azure::elsewhere(everywhere, size, region);
    if !regions.is_empty() && refused * 5 < (refused + regions.len()) * 4 {
        regions.truncate(SUGGESTED);
        return format!(" This subscription can use it in {}.", listed(&regions));
    }
    if refused > 0 {
        return sizes(" This subscription is refused it in almost every region, as student and free subscriptions often are.");
    }
    sizes(" No other region offers it to this subscription.")
}

/// "a", "a or b", "a, b or c".
fn listed(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// The VM size check from the family's quota ([`azure::quotas`]); one that can't be had says why.
fn quota_check(quotas: &Result<Vec<azure::Quota>, Failure>, family: &str, size: &str, region: &str) -> Check {
    let quotas = match quotas {
        Ok(quotas) => quotas,
        Err(why) => return Check::warn("VM size", format!("Couldn't read the core quota for {size} in {region} ({why}); a launch may still fail on it.")),
    };
    match azure::free_cores(quotas, family) {
        Some(f) if f >= 2 => Check::ok("VM size", format!("{size} is available in {region} ({f} cores of quota free).")),
        Some(_) => {
            Check::fail("VM size", format!("No core quota left for {size} in {region}. Request more in the Azure portal (Quotas), or try another region."))
        }
        None => Check::warn("VM size", format!("Couldn't read the core quota for {size} in {region}; a launch may still fail on it.")),
    }
}

/// The sizes lists read this session, by subscription and what was asked: Azure's answer
/// changes when a subscription is granted a size, rarely, and a minute per read is felt.
static READ: Mutex<Option<HashMap<String, Arc<Vec<Size>>>>> = Mutex::new(None);

/// [`azure::sizes`] for `subscription`, of `region` or (None) of every region, narrowed to `size`
/// when given; kept for the session once read. A failure (signed out, throttled, no network, an
/// answer that isn't the list) is an error, never an empty list, and isn't kept.
async fn list_sizes(subscription: &str, region: Option<&str>, size: Option<&str>) -> Result<Arc<Vec<Size>>, Failure> {
    let key = format!("{subscription}|{}|{}", region.unwrap_or("*"), size.unwrap_or("*"));
    if let Some(hit) = READ.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_default().get(&key) {
        return Ok(hit.clone());
    }
    let read = Arc::new(azure::sizes(&machine(), subscription, region, size).await?);
    READ.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_default().insert(key, read.clone());
    Ok(read)
}

/// Azure: whether this subscription can create the lab VM size in the account's region, with its
/// quota; when it can't, why (not offered there, or not to this subscription) and what to try.
pub(super) async fn capacity_check(h: &HostProfile) -> Check {
    // The size labs use: the one set on the account, else Isoloom's default for a small lab.
    let size = h.size().unwrap_or(DEFAULT_SIZE);
    let (region, subscription) = (h.region().unwrap_or_default().to_lowercase(), h.cloud_account().unwrap_or_default());
    let cx = machine();
    let (region_sizes, quotas) = tokio::join!(list_sizes(subscription, Some(&region), None), azure::quotas(&cx, subscription, &region));
    let region_sizes = match region_sizes {
        Ok(sizes) => sizes,
        Err(why) => return Check::warn("VM size", format!("Couldn't ask Azure whether {size} is offered in {region}: {why}")),
    };
    let fault = match azure::in_region(&region_sizes, size) {
        InRegion::Usable(family) => return quota_check(&quotas, &family, size, &region),
        InRegion::Restricted(why) if why == "QuotaId" => format!("{size} is offered in {region}, but not to this kind of subscription (Azure: QuotaId)."),
        InRegion::Restricted(why) => format!("{size} is offered in {region}, but not to this subscription (Azure: {why})."),
        // Not offered there.
        _ => format!("{size} isn't offered in {region}."),
    };
    let everywhere = list_sizes(subscription, None, Some(size)).await.ok();
    Check::fail("VM size", fault + &suggestion(size, &region, everywhere.as_deref().map(Vec::as_slice), &region_sizes))
}

#[cfg(test)]
mod tests;
