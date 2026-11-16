use super::{ipv4_subnets, networks_of_others};

#[test]
fn a_labs_own_networks_are_told_by_their_exact_project() {
    // Regression: a lab's own networks were picked by name prefix, so lab `web` also took
    // lab `web-2`'s networks for its own, and a new instance could land on their subnets.
    let listed = "bridge\t\ncyberctf-web_lab\tcyberctf-web\ncyberctf-web-2_lab\tcyberctf-web-2\nhost\t\n\n";
    assert_eq!(networks_of_others(listed, "cyberctf-web"), ["bridge", "cyberctf-web-2_lab", "host"]);
    assert_eq!(networks_of_others(listed, "cyberctf-web-2"), ["bridge", "cyberctf-web_lab", "host"]);
    assert_eq!(networks_of_others("plain\n", "cyberctf-web"), ["plain"]);
}

#[test]
fn network_subnets_skip_ipv6() {
    assert_eq!(ipv4_subnets("172.17.0.0/16 fd00::/64\n10.30.0.0/24 \n"), ["172.17.0.0/16", "10.30.0.0/24"]);
}
