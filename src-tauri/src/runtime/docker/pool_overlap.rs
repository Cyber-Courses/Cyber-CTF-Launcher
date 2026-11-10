//! Docker's address-pool clash, said as what to do.

use crate::error::Error;

/// Docker's "Pool overlaps with other one on this address space", said as what to do: another
/// network (a lab, or one left behind) holds an address block this lab needs.
pub(crate) fn explain_pool_overlap(e: Error) -> Error {
    if !e.to_string().contains("Pool overlaps") {
        return e;
    }
    Error::Invalid(format!(
        "Another Docker network already uses the addresses this lab needs. Stop the other lab, or remove the network left behind (`docker network prune` removes the unused ones), then start this lab again. ({e})"
    ))
}

#[cfg(test)]
mod tests {
    use super::explain_pool_overlap;

    #[test]
    fn a_pool_overlap_says_what_to_do() {
        use crate::error::Error;
        let raw = Error::CommandFailed {
            command: "docker compose up".into(),
            stderr: "Error response from daemon: invalid pool request: Pool overlaps with other one on this address space".into(),
        };
        let said = explain_pool_overlap(raw).to_string();
        assert!(said.starts_with("Another Docker network already uses the addresses this lab needs.") && said.contains("Pool overlaps"), "{said}");
        assert_eq!(explain_pool_overlap(Error::Invalid("port taken".into())).to_string(), "port taken");
    }
}
