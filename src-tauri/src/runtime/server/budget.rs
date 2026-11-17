//! What an AWS account has cost this month (Cost Explorer), and the budget check run before a
//! cloud launch. AWS-only: the other clouds have no cost read yet, and never persist a limit.

use isothrum_machine::cloud::aws;
use isothrum_machine::{Failure, Refusal};

use crate::cloud_cli::{machine, tool_error};
use crate::platform::AppDirs;

use super::cloud_checks::{aws_account, last_line, reason};
use super::profile::HostProfile;
use super::secrets::host_secret;
use super::store::load_host;
use crate::error::{Error, Result};
use crate::runtime::providers::Provider;

/// This month's AWS spend as the AWS CLI's `profile` (or its default) sees it: the setup form's,
/// before an account in CLI mode is saved.
pub async fn cli_month_to_date_cost(profile: Option<String>) -> std::result::Result<f64, Failure> {
    month_to_date_cost(&cli_account(profile), "").await
}

/// An AWS account signed in through the CLI's `profile`. Cost Explorer answers in us-east-1
/// whatever the account's region.
fn cli_account(profile: Option<String>) -> HostProfile {
    HostProfile {
        id: String::new(),
        name: String::new(),
        provider: Provider::Aws,
        host: "us-east-1".into(),
        port: super::profile::default_port(Provider::Aws),
        username: String::new(),
        datastore: None,
        network: None,
        node: None,
        insecure_tls: false,
        tls_fingerprint: None,
        auto_stop_hours: None,
        use_cli_creds: true,
        aws_profile: profile,
        monthly_limit: None,
        gcp_project: None,
        ssh_key: None,
        image_storage: None,
        snippet_storage: None,
    }
}

/// This month's spend on an account with a budget, as the Cloud page shows it: what it has cost
/// so far, and whether that reached the budget (the launch gate's verdict).
#[derive(Debug, PartialEq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MonthSpend {
    /// This month's spend so far, in USD.
    pub spent: f64,
    /// Whether it reached the account's monthly budget: a cloud launch on it is refused.
    pub over: bool,
}

/// This month's spend so far on the saved account `id`, with its own credentials (its stored
/// keys, or its CLI profile), against its budget: the figure the Cloud screen shows is the one
/// the launch budget gate checks, never another account's (the CLI's default profile's). Only an
/// account with a budget ([`budget_limit`]) is asked: Cost Explorer bills each call.
pub(crate) async fn account_month_spend(dirs: &AppDirs, id: &str) -> Result<MonthSpend> {
    let host = load_host(dirs, id)?;
    let Some(limit) = budget_limit(&host) else {
        return Err(Error::Invalid("only an AWS account with a monthly budget reports its spend".into()));
    };
    let spent = month_to_date_cost(&host, &host_secret(&host)?).await.map_err(|e| tool_error(&e))?;
    Ok(MonthSpend { spent, over: reached(limit, spent) })
}

/// This month's AWS spend so far (USD) for a host, via Cost Explorer, with its own credentials.
pub(super) async fn month_to_date_cost(h: &HostProfile, password: &str) -> std::result::Result<f64, Failure> {
    aws::month_to_date_cost(&machine(), &aws_account(h, password)).await
}

/// The budget check for a launch. Blocks only when the spend is readable and over the limit;
/// when it can't be read (`Unverifiable`) the caller warns and launches anyway (best effort).
pub enum BudgetCheck {
    /// No budget set, under it, or not an AWS account.
    Ok,
    /// At or over the monthly budget: (spent, limit) in USD.
    Over(f64, f64),
    /// A budget is set but this month's spend couldn't be read (Cost Explorer off, no
    /// `ce:GetCostAndUsage`, Docker/CLI missing, transient error). Carries why.
    Unverifiable(String),
}

/// Checks an account against its monthly budget before a launch. An account that can't be read,
/// or whose stored keys can't be, is `Unverifiable` (saying why), never under budget.
pub async fn check_budget(dirs: &AppDirs, id: &str) -> BudgetCheck {
    let host = match load_host(dirs, id) {
        Ok(host) => host,
        Err(e) => return BudgetCheck::Unverifiable(format!("couldn't read this account ({e}).")),
    };
    let Some(limit) = budget_limit(&host) else { return BudgetCheck::Ok };
    let password = match host_secret(&host) {
        Ok(password) => password,
        Err(e) => return BudgetCheck::Unverifiable(keys_unreadable_message(&e)),
    };
    verdict(limit, month_to_date_cost(&host, &password).await)
}

/// Why an account's stored keys couldn't be read for the budget check: none stored, or the
/// keychain's own reason.
fn keys_unreadable_message(e: &Error) -> String {
    if e.code() == crate::error::Code::NoPasswordStored {
        "No stored credentials for this account. Re-enter its access keys, then launch again.".into()
    } else {
        format!("couldn't read this account's stored keys ({e}).")
    }
}

/// The monthly limit a launch is checked against: AWS accounts with a positive limit only. The
/// webview reads it from the saved list (`ServerList::budgets`).
pub(crate) fn budget_limit(host: &HostProfile) -> Option<f64> {
    if host.provider != Provider::Aws {
        return None;
    }
    host.monthly_limit.filter(|v| *v > 0.0)
}

/// Whether `spent` reached the budget `limit`: from there, a cloud launch is refused.
fn reached(limit: f64, spent: f64) -> bool {
    spent >= limit
}

/// This month's spend (or why it couldn't be read) against `limit`.
fn verdict(limit: f64, spent: std::result::Result<f64, Failure>) -> BudgetCheck {
    match spent {
        Ok(spent) if spent.is_nan() => BudgetCheck::Unverifiable("couldn't read this month's spend (the spend figure isn't a number).".into()),
        Ok(spent) if reached(limit, spent) => BudgetCheck::Over(spent, limit),
        Ok(_) => BudgetCheck::Ok,
        Err(e) => BudgetCheck::Unverifiable(budget_unverifiable_message(&e)),
    }
}

/// Turns a Cost Explorer failure into actionable guidance. An expired / missing sign-in is the
/// common case (and the launch would fail on it too), so it gets a "reauthenticate" message
/// rather than the misleading "enable Cost Explorer".
pub(super) fn budget_unverifiable_message(failure: &Failure) -> String {
    match failure.refusal() {
        Some(Refusal::CredentialsExpired) => "the AWS session has expired or isn't signed in.".into(),
        Some(Refusal::NotEnabled) => "Cost Explorer isn't enabled on this account (enable it in the AWS Billing console).".into(),
        _ => format!("couldn't read this month's spend ({}).", last_line(&reason(failure))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::profile::tests::profile;

    #[tokio::test]
    async fn an_accounts_spend_is_read_with_its_own_keys_not_the_clis_default() {
        // Regression: the Cloud screen read the spend of an account added with stored keys through
        // the host CLI's default profile: another account's spend, or nothing, while the launch
        // budget gate read the stored keys.
        use crate::platform::exec::fake::{Tools, ok};
        let t = crate::testing::test_app::TestApp::new();
        let cli = t.host("aws-cli", "aws", serde_json::json!({ "useCliCreds": true, "awsProfile": "labs", "monthlyLimit": 10.0 }));
        let tools = Tools::expect(&[("aws ce get-cost-and-usage", ok("7.25\n"))]);
        assert_eq!(account_month_spend(&t.dirs(), &cli).await.unwrap(), MonthSpend { spent: 7.25, over: false });
        assert_eq!(tools.env_of("ce get-cost-and-usage", "AWS_PROFILE").as_deref(), Some("labs"));
        // At the budget, the verdict the launch gate gives: over.
        let tight = t.host("aws-tight", "aws", serde_json::json!({ "useCliCreds": true, "monthlyLimit": 7.25 }));
        assert_eq!(account_month_spend(&t.dirs(), &tight).await.unwrap(), MonthSpend { spent: 7.25, over: true });
        // No budget (none set, or not an AWS account), or not saved: no figure, Cost Explorer not asked.
        let asked = tools.calls().len();
        let free = t.host("aws-free", "aws", serde_json::json!({ "useCliCreds": true }));
        let az = t.host("az1", "azure", serde_json::json!({ "monthlyLimit": 10.0 }));
        for id in [&free, &az] {
            assert_eq!(account_month_spend(&t.dirs(), id).await.unwrap_err().to_string(), "only an AWS account with a monthly budget reports its spend");
        }
        assert!(account_month_spend(&t.dirs(), "nope").await.is_err());
        assert_eq!(tools.calls().len(), asked);
        // The setup form's figure, for a profile not saved yet.
        crate::testing::assert_close(cli_month_to_date_cost(None).await.unwrap(), 7.25);
    }

    #[tokio::test]
    async fn an_account_that_cant_be_read_is_unverifiable_never_under_budget() {
        // Regression: an account missing from (or unreadable in) the saved hosts read as under
        // budget, and its launch went ahead unchecked and unsaid.
        let t = crate::testing::test_app::TestApp::new();
        match check_budget(&t.dirs(), "gone").await {
            BudgetCheck::Unverifiable(why) => assert!(why.starts_with("couldn't read this account ("), "{why}"),
            _ => panic!("should be unverifiable"),
        }
        // An account without a limit is never checked.
        let free = t.host("aws-free", "aws", serde_json::json!({ "useCliCreds": true }));
        assert!(matches!(check_budget(&t.dirs(), &free).await, BudgetCheck::Ok));
    }

    #[test]
    fn keys_that_cant_be_read_say_why_and_only_missing_ones_ask_to_re_enter_them() {
        // Regression: a locked keychain read as keys never entered.
        let none = keys_unreadable_message(&Error::coded(crate::error::Code::NoPasswordStored));
        assert!(none.starts_with("No stored credentials for this account."), "{none}");
        let locked = keys_unreadable_message(&Error::Invalid("keychain: the keychain is locked".into()));
        assert_eq!(locked, "couldn't read this account's stored keys (keychain: the keychain is locked).");
    }

    #[test]
    fn stored_keys_reach_cost_explorer_in_the_cli_container() {
        let keys = HostProfile { host: "eu-west-3".into(), username: "AKIAIOSFODNN7EXAMPLE".into(), ..profile(Provider::Aws) };
        let (program, _, env) = aws_account(&keys, "secret").invocation(&["ce"]);
        assert_eq!(program, "docker");
        assert!(env.contains(&("AWS_ACCESS_KEY_ID".to_string(), "AKIAIOSFODNN7EXAMPLE".to_string())));
        assert!(cli_account(Some("p".into())).use_cli_creds && cli_account(None).aws_profile.is_none());
    }

    #[tokio::test]
    async fn a_spend_cost_explorer_answers_in_words_is_unverifiable_never_under_budget() {
        use crate::platform::exec::fake::{Tools, ok};
        for answer in ["None", "NaN", "12,5"] {
            let _tools = Tools::expect(&[("aws ce get-cost-and-usage", ok(answer))]);
            assert!(matches!(verdict(40.0, cli_month_to_date_cost(None).await), BudgetCheck::Unverifiable(_)), "{answer}");
        }
        // An infinite spend is over any limit.
        assert!(matches!(verdict(40.0, Ok(f64::INFINITY)), BudgetCheck::Over(..)));
        assert!(matches!(verdict(40.0, Ok(f64::NAN)), BudgetCheck::Unverifiable(_)));
    }

    #[test]
    fn only_aws_accounts_with_a_positive_limit_are_checked() {
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(40.0), ..profile(Provider::Aws) }), Some(40.0));
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(0.0), ..profile(Provider::Aws) }), None);
        assert_eq!(budget_limit(&profile(Provider::Aws)), None);
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(40.0), ..profile(Provider::Azure) }), None);
    }

    /// AWS refusing the spend, saying `said`, read as isothrum-machine reads it.
    fn refused(said: &str) -> Failure {
        Failure::Refused { program: "aws".into(), refusal: aws::refusal(said), said: said.lines().last().unwrap_or_default().into() }
    }

    #[test]
    fn the_verdict_blocks_only_a_readable_spend_over_the_limit() {
        assert!(matches!(verdict(40.0, Ok(40.0)), BudgetCheck::Over(s, l) if (s - 40.0).abs() < f64::EPSILON && (l - 40.0).abs() < f64::EPSILON));
        assert!(matches!(verdict(40.0, Ok(39.99)), BudgetCheck::Ok));
        match verdict(
            40.0,
            Err(refused("An error occurred (AccessDeniedException) when calling the GetCostAndUsage operation: User not enabled for cost explorer access")),
        ) {
            BudgetCheck::Unverifiable(why) => assert!(why.contains("Cost Explorer isn't enabled"), "{why}"),
            _ => panic!("should be unverifiable"),
        }
    }

    #[test]
    fn budget_message_calls_out_an_expired_session_first() {
        // Auth-shaped failures win, whatever else is in the text.
        for err in ["The security token included in the request is expired", "Unable to locate credentials", "Error loading SSO Token", "InvalidClientTokenId"]
        {
            assert_eq!(budget_unverifiable_message(&refused(err)), "the AWS session has expired or isn't signed in.", "{err}");
        }
    }

    #[test]
    fn budget_message_explains_cost_explorer_when_disabled() {
        let msg = budget_unverifiable_message(&refused("Cost Explorer is not enabled for this account"));
        assert!(msg.contains("Cost Explorer isn't enabled"), "{msg}");
    }

    #[test]
    fn budget_message_falls_back_to_the_last_line() {
        let msg = budget_unverifiable_message(&refused("something odd\nthe real reason here"));
        assert_eq!(msg, "couldn't read this month's spend (the real reason here).");
        let missing = budget_unverifiable_message(&Failure::Missing { program: "docker".into() });
        assert!(missing.starts_with("couldn't read this month's spend ("), "{missing}");
    }
}
