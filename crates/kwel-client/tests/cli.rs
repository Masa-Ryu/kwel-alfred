use kwel_client::{Operation, parse_args};
use kwel_protocol::LocalRequest;
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
#[test]
fn json_cli_preserves_raw_arguments_and_normalizes_profile() {
    for query in ["Kyoto", "日本語 ⚙", "quotes '\" $() ! ;", "--limit"] {
        let args = strings(&[
            "search",
            "--profile-id",
            " 00000000-0000-4000-8000-00000000000A ",
            "--query",
            query,
            "--format",
            "json",
        ]);
        assert!(
            matches!(parse_args(&args,"json").unwrap(),Operation::Request(LocalRequest::Search{query:actual,profile_id,limit:20,..}) if actual==query&&profile_id=="00000000-0000-4000-8000-00000000000a")
        );
    }
}
#[test]
fn invalid_cli_never_becomes_a_management_or_url_open_request() {
    for args in [
        strings(&["doctor"]),
        strings(&["uninstall"]),
        strings(&["search", "--query", "Kyoto"]),
        strings(&[
            "search",
            "--profile-id",
            "00000000-0000-4000-8000-00000000000a",
            "--format",
            "alfred",
        ]),
        strings(&[
            "open",
            "--profile-id",
            "00000000-0000-4000-8000-00000000000a",
            "--ref",
            "https://example.com",
        ]),
    ] {
        assert!(parse_args(&args, "json").is_err());
    }
}
