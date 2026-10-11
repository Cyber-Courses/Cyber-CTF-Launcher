//! What an AWS account has cost this month (Cost Explorer), and the budget check run before a
//! cloud launch. AWS-only: the other clouds have no cost read yet, and never persist a limit.

use crate::AppHandle;

use super::cloud_checks::{aws_cmd, last_line};
use super::contract::terraform_env;
use super::profile::HostProfile;
use super::secrets::host_secret;
use super::store::load_host;
use crate::error::{Error, Result};
use crate::runtime::providers::Provider;

/// This month's AWS spend so far (USD) for a host, via Cost Explorer.
pub(super) async fn month_to_date_cost(h: &HostProfile, password: &str) -> Result<f64> {
    let env = terraform_env(h, password);
    let period = crate::cloud::month_period();
    let out = aws_cmd(h, &env, &cost_args(&period)).await?;
    parse_spend(&out)
}

/// `aws ce get-cost-and-usage` for `period`, printing just the amount.
fn cost_args(period: &str) -> [&str; 12] {
    [
        "ce",
        "get-cost-and-usage",
        "--time-period",
        period,
        "--granularity",
        "MONTHLY",
        "--metrics",
        "UnblendedCost",
        "--query",
        "ResultsByTime[0].Total.UnblendedCost.Amount",
        "--output",
        "text",
    ]
}

/// Cost Explorer's amount (text output) as a number.
fn parse_spend(out: &str) -> Result<f64> {
    out.trim().parse::<f64>().map_err(|_| Error::Invalid(format!("couldn't read the spend figure from Cost Explorer: {:?}", out.trim())))
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

/// Checks an account against its monthly budget before a launch.
pub async fn check_budget(app: &AppHandle, id: &str) -> BudgetCheck {
    let Ok(host) = load_host(app, id) else { return BudgetCheck::Ok };
    let Some(limit) = budget_limit(&host) else { return BudgetCheck::Ok };
    let Ok(password) = host_secret(&host) else {
        return BudgetCheck::Unverifiable("No stored credentials for this account. Re-enter its access keys, then launch again.".into());
    };
    verdict(limit, month_to_date_cost(&host, &password).await)
}

/// The monthly limit a launch is checked against: AWS accounts with a positive limit only.
fn budget_limit(host: &HostProfile) -> Option<f64> {
    if host.provider != Provider::Aws {
        return None;
    }
    host.monthly_limit.filter(|v| *v > 0.0)
}

/// This month's spend (or why it couldn't be read) against `limit`.
fn verdict(limit: f64, spent: Result<f64>) -> BudgetCheck {
    match spent {
        Ok(spent) if spent >= limit => BudgetCheck::Over(spent, limit),
        Ok(_) => BudgetCheck::Ok,
        Err(e) => BudgetCheck::Unverifiable(budget_unverifiable_message(&e.to_string())),
    }
}

/// Turns a Cost Explorer failure into actionable guidance. An expired / missing sign-in is the
/// common case (and the launch would fail on it too), so it gets a "reauthenticate" message
/// rather than the misleading "enable Cost Explorer".
pub(super) fn budget_unverifiable_message(err: &str) -> String {
    let lower = err.to_lowercase();
    let auth = ["expired", "aws login", "invalidclienttokenid", "unable to locate credentials", "sso", "not logged in", "tokenrefresh", "credentials"]
        .iter()
        .any(|m| lower.contains(m));
    if auth {
        "the AWS session has expired or isn't signed in.".into()
    } else if lower.contains("cost explorer") || lower.contains("not enabled") {
        "Cost Explorer isn't enabled on this account (enable it in the AWS Billing console).".into()
    } else {
        format!("couldn't read this month's spend ({}).", last_line(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::profile::tests::profile;

    #[test]
    fn spend_figures_parse_or_say_what_came_back() {
        assert_eq!(parse_spend("12.5\n").unwrap(), 12.5);
        assert_eq!(parse_spend(" 0 ").unwrap(), 0.0);
        assert_eq!(parse_spend("None\n").unwrap_err().to_string(), "couldn't read the spend figure from Cost Explorer: \"None\"");
    }

    #[test]
    fn cost_explorer_is_asked_for_this_period_only() {
        let args = cost_args("Start=2026-10-01,End=2026-10-11");
        assert_eq!(args[..4], ["ce", "get-cost-and-usage", "--time-period", "Start=2026-10-01,End=2026-10-11"]);
        assert_eq!(args[args.len() - 2..], ["--output", "text"]);
    }

    #[test]
    fn only_aws_accounts_with_a_positive_limit_are_checked() {
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(40.0), ..profile(Provider::Aws) }), Some(40.0));
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(0.0), ..profile(Provider::Aws) }), None);
        assert_eq!(budget_limit(&profile(Provider::Aws)), None);
        assert_eq!(budget_limit(&HostProfile { monthly_limit: Some(40.0), ..profile(Provider::Azure) }), None);
    }

    #[test]
    fn the_verdict_blocks_only_a_readable_spend_over_the_limit() {
        assert!(matches!(verdict(40.0, Ok(40.0)), BudgetCheck::Over(s, l) if s == 40.0 && l == 40.0));
        assert!(matches!(verdict(40.0, Ok(39.99)), BudgetCheck::Ok));
        match verdict(40.0, Err(Error::Invalid("Cost Explorer is not enabled".into()))) {
            BudgetCheck::Unverifiable(why) => assert!(why.contains("Cost Explorer isn't enabled"), "{why}"),
            _ => panic!("should be unverifiable"),
        }
    }

    #[test]
    fn budget_message_calls_out_an_expired_session_first() {
        // Auth-shaped failures win, whatever else is in the text.
        for err in ["The security token included in the request is expired", "Unable to locate credentials", "Error loading SSO Token", "InvalidClientTokenId"]
        {
            assert_eq!(budget_unverifiable_message(err), "the AWS session has expired or isn't signed in.", "{err}");
        }
    }

    #[test]
    fn budget_message_explains_cost_explorer_when_disabled() {
        let msg = budget_unverifiable_message("Cost Explorer is not enabled for this account");
        assert!(msg.contains("Cost Explorer isn't enabled"), "{msg}");
    }

    #[test]
    fn budget_message_falls_back_to_the_last_line() {
        let msg = budget_unverifiable_message("something odd\nthe real reason here");
        assert!(msg.contains("the real reason here"), "{msg}");
    }
}
