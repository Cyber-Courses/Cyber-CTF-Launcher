use serde_json::json;

use super::*;
use crate::error::Error;
use crate::platform::exec::fake::{Tools, fail, ok};
use crate::runtime::{
    providers::Provider,
    server::{checks::CheckState, profile::tests::profile},
};

const SIZE: &str = "Standard_D2als_v6";

/// An Azure account in `region` on `subscription` (each test its own, as the sizes lists are kept
/// per subscription for the session), with no size of its own.
fn account(subscription: &str, region: &str) -> HostProfile {
    HostProfile { host: region.into(), username: subscription.into(), datastore: None, ..profile(Provider::Azure) }
}

/// A size in `location` as `az vm list-skus --all` lists it, refused to the subscription there
/// for `reason` when given.
fn entry(name: &str, location: &str, reason: Option<&str>, vcpus: u32, memory: u32) -> serde_json::Value {
    let restrictions = reason.map_or_else(Vec::new, |r| {
        vec![json!({"type": "Location", "reasonCode": r, "values": [location], "restrictionInfo": {"locations": [location], "zones": null}})]
    });
    json!({
        "name": name, "family": "standardDALSv6Family", "resourceType": "virtualMachines",
        "locations": [location], "locationInfo": [{"location": location, "zones": ["1", "2", "3"]}],
        "restrictions": restrictions,
        "capabilities": [{"name": "vCPUs", "value": vcpus.to_string()}, {"name": "MemoryGB", "value": memory.to_string()}],
    })
}

const USAGE: &str = r#"[{"name": {"value": "standardDALSv6Family"}, "limit": "10", "currentValue": "4"}]"#;

/// `az` answering the region's sizes, the size's across every region, and the quotas.
fn azure(region: &[serde_json::Value], everywhere: &[serde_json::Value]) -> Tools {
    let (region, everywhere) = (json!(region).to_string(), json!(everywhere).to_string());
    Tools::new(move |program, args| match (program, args) {
        ("az", ["vm", "list-skus", rest @ ..]) if rest.contains(&"-l") => ok(region.clone()),
        ("az", ["vm", "list-skus", ..]) => ok(everywhere.clone()),
        ("az", ["vm", "list-usage", ..]) => ok(USAGE),
        _ => None,
    })
}

/// The check's detail after its first sentence, the one that says why: what it suggests.
fn suggested(c: &Check) -> &str {
    c.detail.split_once(". ").map_or("", |(_, rest)| rest)
}

#[tokio::test]
async fn a_size_this_subscription_can_use_passes_on_its_quota_and_its_region_is_read_once() {
    let tools = azure(&[entry(SIZE, "swedencentral", None, 2, 4), entry("Standard_B2s", "swedencentral", None, 2, 4)], &[]);
    let c = capacity_check(&account("sub-ok", "swedencentral")).await;
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "Standard_D2als_v6 is available in swedencentral (6 cores of quota free)."));
    // `--all`: without it az leaves out the sizes refused to the subscription, which then read as
    // not offered. Narrowed to the region, the one filter Azure applies itself.
    assert!(tools.called("vm list-skus --subscription sub-ok --resource-type virtualMachines --all -o json -l swedencentral"), "{:?}", tools.calls());
    assert!(!tools.calls().iter().any(|c| c.contains("list-skus") && !c.contains("-l")), "the size everywhere isn't read when it can be had here");
    // Tested again this session: the region's sizes aren't read again (a minute each).
    capacity_check(&account("sub-ok", "swedencentral")).await;
    assert_eq!(tools.calls().iter().filter(|c| c.contains("list-skus")).count(), 1, "{:?}", tools.calls());
}

#[tokio::test]
async fn a_size_not_offered_in_the_region_names_regions_that_offer_it_never_the_region_checked() {
    // Regression: "Standard_D2als_v6 isn't offered in swedencentral. Try the region swedencentral".
    let everywhere = vec![
        entry(SIZE, "westeurope", None, 2, 4),
        entry(SIZE, "northeurope", None, 2, 4),
        entry(SIZE, "francecentral", None, 2, 4),
        entry(SIZE, "uksouth", None, 2, 4),
    ];
    let tools = azure(&[entry("Standard_B2s", "swedencentral", None, 2, 4)], &everywhere);
    let c = capacity_check(&account("sub-not-offered", "swedencentral")).await;
    assert_eq!(c.state, CheckState::Fail);
    assert_eq!(c.detail, "Standard_D2als_v6 isn't offered in swedencentral. This subscription can use it in francecentral, northeurope or uksouth.");
    assert!(tools.called("--size Standard_D2als_v6"), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_size_refused_to_the_subscription_says_so_and_where_it_isnt() {
    let region = vec![entry(SIZE, "swedencentral", Some("NotAvailableForSubscription"), 2, 4)];
    let everywhere = vec![
        entry(SIZE, "swedencentral", Some("NotAvailableForSubscription"), 2, 4),
        entry(SIZE, "westeurope", None, 2, 4),
        entry(SIZE, "northeurope", None, 2, 4),
    ];
    let _tools = azure(&region, &everywhere);
    let c = capacity_check(&account("sub-refused", "swedencentral")).await;
    assert_eq!(c.state, CheckState::Fail);
    assert_eq!(
        c.detail,
        "Standard_D2als_v6 is offered in swedencentral, but not to this subscription (Azure: NotAvailableForSubscription). This subscription can use it in northeurope or westeurope."
    );
}

#[tokio::test]
async fn a_size_refused_almost_everywhere_names_sizes_the_subscription_can_use_in_the_region() {
    // A student subscription: the size is refused in nine regions of ten.
    let mut everywhere: Vec<_> = ["swedencentral", "westeurope", "northeurope", "francecentral", "uksouth", "eastus", "eastus2", "westus", "centralus"]
        .iter()
        .map(|l| entry(SIZE, l, Some("NotAvailableForSubscription"), 2, 4))
        .collect();
    everywhere.push(entry(SIZE, "koreacentral", None, 2, 4));
    let region = vec![
        entry(SIZE, "swedencentral", Some("NotAvailableForSubscription"), 2, 4),
        entry("Standard_D2s_v5", "swedencentral", None, 2, 8),
        entry("Standard_B2s", "swedencentral", None, 2, 4),
        entry("Standard_B1s", "swedencentral", None, 1, 1),
        entry("Standard_D2as_v5", "swedencentral", Some("NotAvailableForSubscription"), 2, 8),
        entry("Standard_D4s_v5", "swedencentral", None, 4, 16),
        entry("Standard_E2s_v5", "swedencentral", None, 2, 16),
    ];
    let _tools = azure(&region, &everywhere);
    let c = capacity_check(&account("sub-student", "swedencentral")).await;
    assert_eq!(c.state, CheckState::Fail);
    assert_eq!(
        suggested(&c),
        "This subscription is refused it in almost every region, as student and free subscriptions often are. Sizes it can use in swedencentral: Standard_B2s, Standard_D2s_v5, Standard_E2s_v5."
    );
}

#[tokio::test]
async fn a_size_refused_to_this_kind_of_subscription_says_so() {
    let _tools = azure(&[entry(SIZE, "westeurope", Some("QuotaId"), 2, 4)], &[entry(SIZE, "westeurope", Some("QuotaId"), 2, 4)]);
    let c = capacity_check(&account("sub-quota-id", "westeurope")).await;
    assert!(c.detail.starts_with("Standard_D2als_v6 is offered in westeurope, but not to this kind of subscription (Azure: QuotaId)."), "{}", c.detail);
    assert!(!suggested(&c).contains("westeurope"), "{}", c.detail);
}

#[tokio::test]
async fn a_size_not_offered_anywhere_says_so_with_the_regions_sizes_and_never_the_region_checked() {
    let _tools = azure(&[entry("Standard_B2s", "westeurope", None, 2, 4)], &[]);
    let c = capacity_check(&account("sub-nowhere", "westeurope")).await;
    assert_eq!(
        c.detail,
        "Standard_D2als_v6 isn't offered in westeurope. No other region offers it to this subscription. Sizes it can use in westeurope: Standard_B2s."
    );
}

#[tokio::test]
async fn a_size_list_across_regions_that_cant_be_read_still_names_the_regions_sizes() {
    let _tools = Tools::new(|program, args| match (program, args) {
        ("az", ["vm", "list-skus", rest @ ..]) if rest.contains(&"-l") => ok(json!([entry("Standard_B2s", "westeurope", None, 2, 4)]).to_string()),
        ("az", ["vm", "list-skus", ..]) => fail("throttled"),
        ("az", ["vm", "list-usage", ..]) => ok(USAGE),
        _ => None,
    });
    let c = capacity_check(&account("sub-throttled", "westeurope")).await;
    assert_eq!(c.detail, "Standard_D2als_v6 isn't offered in westeurope. Sizes this subscription can use in westeurope: Standard_B2s.");
}

#[tokio::test]
async fn an_azure_cli_that_fails_says_it_couldnt_ask_never_that_the_size_isnt_offered() {
    // Regression: a failed `az vm list-skus` (signed out, throttled, a bad region) read as an
    // empty list, and the check said the size isn't offered there.
    let _tools = Tools::rules(&[("list-skus", fail("ERROR: Please run 'az login' to setup account.")), ("list-usage", ok(USAGE))]);
    let c = capacity_check(&account("sub-signed-out", "westeurope")).await;
    assert_eq!(c.state, CheckState::Warn);
    assert!(c.detail.starts_with("Couldn't ask Azure whether Standard_D2als_v6 is offered in westeurope: "), "{}", c.detail);
    assert!(c.detail.contains("az login"), "{}", c.detail);
    // Not kept: the next test asks again.
    let _tools = azure(&[entry(SIZE, "westeurope", None, 2, 4)], &[]);
    assert_eq!(capacity_check(&account("sub-signed-out", "westeurope")).await.state, CheckState::Ok);
}

#[test]
fn only_a_location_restriction_covering_the_region_refuses_a_size() {
    let zones = json!([{
        "name": SIZE, "family": "f", "locations": ["westeurope"],
        "restrictions": [{"type": "Zone", "reasonCode": "NotAvailableForSubscription", "values": ["westeurope"], "restrictionInfo": {"locations": ["westeurope"], "zones": ["1"]}}]
    }]);
    assert_eq!(in_region(&skus(&zones.to_string()).unwrap(), SIZE), InRegion::Usable("f".into()));
    let elsewhere = json!([{"name": SIZE, "family": "f", "locations": ["westeurope"], "restrictions": [{"type": "Location", "reasonCode": "NotAvailableForSubscription", "values": ["eastus"]}]}]);
    assert_eq!(in_region(&skus(&elsewhere.to_string()).unwrap(), SIZE), InRegion::Usable("f".into()));
    let here = json!([{"name": SIZE, "family": "f", "locations": ["WestEurope"], "restrictions": [{"type": "Location", "reasonCode": "NotAvailableForSubscription", "values": ["westeurope"]}]}]);
    assert_eq!(in_region(&skus(&here.to_string()).unwrap(), SIZE), InRegion::Restricted("NotAvailableForSubscription".into()));
    assert_eq!(in_region(&[], SIZE), InRegion::NotOffered);
}

#[test]
fn azure_listings_of_the_wrong_shape_or_with_missing_fields_do_not_panic() {
    // The SKU list as an object, a size with null restrictions and no family, and a usage list
    // whose numbers are missing or not numbers.
    assert!(skus("{}").unwrap_err().contains("isn't the list of sizes"));
    assert!(skus("oops").is_err());
    let loose = skus(r#"[{"name": "Standard_D2als_v6", "restrictions": null}, {"no": "name"}]"#).unwrap();
    assert_eq!(in_region(&loose, SIZE), InRegion::Usable(String::new()));
    let usage = json!([
        {"name": {"value": "fam"}, "limit": "10"},
        {"name": {"value": "odd"}, "limit": "ten", "currentValue": 4},
        {"name": "not an object"}
    ]);
    let usage = usage.as_array().unwrap();
    assert_eq!(free_cores(usage, "fam"), Some(10), "no current use yet");
    assert_eq!(free_cores(usage, "odd"), Some(-4), "unreadable numbers count as 0");
    assert_eq!(free_cores(usage, "not an object"), None);
    assert_eq!(quota_check(&Ok("{}".into()), "fam", "s", "r").state, CheckState::Warn);
}

#[test]
fn the_quota_says_how_many_cores_are_free_and_never_names_another_region() {
    let answer = |json: &str| Ok(json.to_owned());
    let usage = answer(r#"[{"name": {"value": "standardDALSv6Family"}, "limit": "10", "currentValue": "4"}]"#);
    let c = quota_check(&usage, "standarddalsv6family", SIZE, "westeurope");
    assert_eq!((c.state, c.detail.as_str()), (CheckState::Ok, "Standard_D2als_v6 is available in westeurope (6 cores of quota free)."));
    let full = answer(r#"[{"name": {"value": "standardDALSv6Family"}, "limit": 4, "currentValue": 3}]"#);
    let c = quota_check(&full, "standarddalsv6family", SIZE, "swedencentral");
    assert_eq!(
        (c.state, c.detail.as_str()),
        (CheckState::Fail, "No core quota left for Standard_D2als_v6 in swedencentral. Request more in the Azure portal (Quotas), or try another region.")
    );
    assert_eq!(quota_check(&usage, "otherfamily", "s", "r").state, CheckState::Warn);
    let c = quota_check(&Err(Error::CommandFailed { command: "az".into(), stderr: "throttled".into() }), "f", SIZE, "westeurope");
    assert_eq!(c.state, CheckState::Warn);
    assert!(c.detail.contains("throttled"), "{}", c.detail);
}

#[test]
fn lists_read_as_a_sentence() {
    assert_eq!(listed(&["a"]), "a");
    assert_eq!(listed(&["a", "b"]), "a or b");
    assert_eq!(listed(&["a", "b", "c"]), "a, b or c");
}
