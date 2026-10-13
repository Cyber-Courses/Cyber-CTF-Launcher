//! The CyberBackend operations the webview may run. `api_query` takes one of these names, never
//! a GraphQL document: any window that can reach the IPC could otherwise run whatever it likes
//! with the player's token.

use serde::Deserialize;

const SESSION_FIELDS: &str = "id labId target state endpoints { port url } message expiresAt";

/// One allowed operation, by the name the frontend sends (`"labs"`, `"labBrief"`, ...).
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ApiOperation {
    /// The published labs, for the Labs screen and Home.
    Labs,
    /// Lab ids and titles, for the Running now panel.
    LabTitles,
    /// The labs the sidebar lists.
    SidebarLabs,
    /// One lab's brief (`$id`).
    LabBrief,
    /// The labs this player has solved.
    CompletedLabs,
    /// The player's active lab session, any target.
    ActiveSession,
    /// Launch a lab on Cyber CTF's hosting (`$labId`).
    LaunchHosted,
    /// One lab session (`$id`).
    LabSession,
    /// Stop a lab session (`$id`).
    StopSession,
    /// One lab's objectives: its challenges and their steps (`$id`).
    LabObjectives,
    /// The player's progress on one lab's objectives (`$id`).
    LabProgress,
    /// Answer one of a lab's challenges or steps (`$labId`, `$key`, `$answer`).
    SubmitObjective,
    /// Open a challenge's steps or one of its hints, which is recorded (`$labId`, `$key`, `$what`, `$index`).
    OpenObjectiveHelp,
}

impl ApiOperation {
    /// The GraphQL document the operation runs.
    pub fn document(self) -> String {
        match self {
            Self::Labs => "{ labs(sort: [{ title: ASC }]) { id slug title description question difficulty category runtime { runtime architectures providers hosted repository commit } skills { id name } } }".into(),
            Self::LabTitles => "{ labs { id title } }".into(),
            Self::SidebarLabs => "{ labs(sort: [{ title: ASC }]) { id slug title category } }".into(),
            Self::LabBrief => "query($id: ID!) { labs(where: { id: { eq: $id } }) { contentMd } }".into(),
            Self::CompletedLabs => "{ myCompletedLabs }".into(),
            Self::ActiveSession => format!("query {{ myActiveLabSession {{ {SESSION_FIELDS} }} }}"),
            Self::LaunchHosted => format!("mutation ($labId: ID!) {{ requestLabLaunch(labId: $labId, target: \"hosted\") {{ {SESSION_FIELDS} }} }}"),
            Self::LabSession => format!("query ($id: ID!) {{ labSession(id: $id) {{ {SESSION_FIELDS} }} }}"),
            Self::StopSession => "mutation ($id: ID!) { stopLabSession(sessionId: $id) { id state } }".into(),
            Self::LabObjectives => "query($id: ID!) { labs(where: { id: { eq: $id } }) { id objectives { key title prompt optional points hintCount steps { key title prompt hintCount } } } }".into(),
            Self::LabProgress => "query($id: ID!) { labs(where: { id: { eq: $id } }) { id myProgress { solved stepsOpened hints { key index text } requiredSolved requiredTotal completed } } }".into(),
            Self::SubmitObjective => "mutation($labId: ID!, $key: String!, $answer: String!) { submitObjective(labId: $labId, key: $key, answer: $answer) { correct alreadySolved labCompleted } }".into(),
            Self::OpenObjectiveHelp => "mutation($labId: ID!, $key: String!, $what: ObjectiveHelpKind!, $index: Int) { openObjectiveHelp(labId: $labId, key: $key, what: $what, index: $index) { ok hint } }".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ApiOperation;

    fn parse(name: &str) -> Result<ApiOperation, serde_json::Error> {
        serde_json::from_value(serde_json::Value::String(name.into()))
    }

    #[test]
    fn the_frontend_names_map_to_their_operations() {
        assert_eq!(parse("labs").unwrap(), ApiOperation::Labs);
        assert_eq!(parse("labBrief").unwrap(), ApiOperation::LabBrief);
        assert_eq!(parse("launchHosted").unwrap(), ApiOperation::LaunchHosted);
        assert_eq!(parse("stopSession").unwrap(), ApiOperation::StopSession);
        assert_eq!(parse("labObjectives").unwrap(), ApiOperation::LabObjectives);
        assert_eq!(parse("labProgress").unwrap(), ApiOperation::LabProgress);
        assert_eq!(parse("submitObjective").unwrap(), ApiOperation::SubmitObjective);
        assert_eq!(parse("openObjectiveHelp").unwrap(), ApiOperation::OpenObjectiveHelp);
    }

    #[test]
    fn a_graphql_document_or_an_unknown_name_is_refused() {
        assert!(parse("{ me { email } }").is_err());
        assert!(parse("deleteAccount").is_err());
        assert!(parse("Labs").is_err());
    }

    #[test]
    fn session_operations_ask_for_the_session_fields() {
        let launch = ApiOperation::LaunchHosted.document();
        assert!(launch.starts_with("mutation ($labId: ID!) { requestLabLaunch(labId: $labId, target: \"hosted\") { id labId"), "{launch}");
        assert!(ApiOperation::ActiveSession.document().contains("myActiveLabSession { id labId target state endpoints { port url } message expiresAt }"));
        assert!(ApiOperation::LabSession.document().contains("labSession(id: $id)"));
    }

    #[test]
    fn objective_operations_run_their_documents() {
        assert!(ApiOperation::LabObjectives.document().contains("objectives { key title prompt optional points hintCount steps"));
        assert!(ApiOperation::LabProgress.document().contains("myProgress { solved stepsOpened hints { key index text }"));
        assert!(ApiOperation::SubmitObjective.document().starts_with("mutation($labId: ID!, $key: String!, $answer: String!) { submitObjective("));
        assert!(ApiOperation::OpenObjectiveHelp.document().contains("openObjectiveHelp(labId: $labId, key: $key, what: $what, index: $index) { ok hint }"));
    }
}
