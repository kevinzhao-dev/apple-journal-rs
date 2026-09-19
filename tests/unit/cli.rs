use super::*;
use clap::Parser;

#[test]
fn db_option_is_global_and_commands_are_typed() {
    let cli = Cli::try_parse_from([
        "journal-rs",
        "list",
        "--db",
        "relative.sqlite",
        "--limit",
        "10",
        "--json",
    ])
    .unwrap();
    assert!(cli.database_path().unwrap().is_absolute());
    assert!(matches!(
        cli.command,
        Command::List(ListRequest {
            output: ListOptions {
                limit: Some(10),
                json: true,
                ..
            },
            ..
        })
    ));
}
#[test]
fn markdown_body_may_start_with_a_dash() {
    let cli = Cli::try_parse_from([
        "journal-rs",
        "render",
        "--body",
        "- alpha\n- beta",
        "--plain",
    ])
    .unwrap();
    assert!(
        matches!(cli.command,Command::Render(RenderRequest{body:Some(ref text),plain:true,..})if text=="- alpha\n- beta")
    );
}
#[test]
fn inappropriate_options_are_rejected_instead_of_ignored() {
    for args in [
        vec!["journal-rs", "list", "--force"],
        vec!["journal-rs", "edit", "1", "--live-photo", "a.jpg", "b.mov"],
        vec!["journal-rs", "write", "--add-media", "x.jpg"],
        vec!["journal-rs", "stats", "extra"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}
#[test]
fn contradictory_edits_and_body_sources_are_rejected() {
    for args in [
        vec!["journal-rs", "edit", "1", "--bookmark", "--no-bookmark"],
        vec![
            "journal-rs",
            "edit",
            "1",
            "--remove-all-media",
            "--remove-media",
            "2",
        ],
        vec![
            "journal-rs",
            "write",
            "--body",
            "text",
            "--body-rtf",
            "x.rtf",
        ],
        vec!["journal-rs", "write", "--markdown", "--body-rtf", "x.rtf"],
        vec!["journal-rs", "render", "--plain", "--inline"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}
#[test]
fn coordinates_are_paired_finite_and_bounded() {
    for (lat, lon) in [("NaN", "0"), ("0", "inf"), ("91", "0"), ("0", "-181")] {
        assert!(Cli::try_parse_from(["journal-rs", "write", "--lat", lat, "--lon", lon]).is_err());
    }
    assert!(Cli::try_parse_from(["journal-rs", "write", "--lat", "0"]).is_err());
    assert!(Cli::try_parse_from(["journal-rs", "write", "--lat", "-90", "--lon", "180"]).is_ok());
}
#[test]
fn location_presentation_requires_coordinates_and_refuses_off() {
    assert!(
        Cli::try_parse_from(["journal-rs", "write", "--location-presentation", "large"]).is_err()
    );
    let error = Cli::try_parse_from([
        "journal-rs",
        "write",
        "--lat",
        "1",
        "--lon",
        "2",
        "--location-presentation",
        "off",
    ])
    .unwrap_err();
    assert!(error.to_string().contains("not supported"));
}
#[test]
fn malformed_dates_limits_and_ids_fail_at_parse_time() {
    for args in [
        vec!["journal-rs", "list", "--limit", "-1"],
        vec!["journal-rs", "list", "--since", "2024-02-31"],
        vec!["journal-rs", "show", "not-an-id"],
        vec!["journal-rs", "edit", "1", "--remove-media", "bad"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}
#[test]
fn media_list_stops_before_following_option() {
    let cli = Cli::try_parse_from([
        "journal-rs",
        "write",
        "--media",
        "a.jpg",
        "b.mov",
        "--bookmark",
    ])
    .unwrap();
    assert!(
        matches!(cli.command,Command::Write(CreateRequest{media,bookmark:true,..})if media.len()==2)
    );
}
