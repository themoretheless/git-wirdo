mod support;
use git_wirdo::app::{Action, App};
use support::TestRepo;

fn lines(first: &str, last: &str) -> String {
    (0..30)
        .map(|i| {
            if i == 1 {
                format!("{first}\n")
            } else if i == 25 {
                format!("{last}\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect()
}

#[test]
fn stage_and_unstage_independent_hunks_keep_working_contents() {
    let repo = TestRepo::new();
    repo.write("[file] name", &lines("first", "last"));
    repo.commit_all("base");
    repo.write("[file] name", &lines("FIRST", "LAST"));
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    let hunks = repository.hunks(&file, false).unwrap();
    assert_eq!(hunks.len(), 2);
    repository.apply_hunk(&hunks[0]).unwrap();
    let index = repo.git(&["show", ":[file] name"]);
    assert!(index.contains("FIRST\n") && index.contains("last\n") && !index.contains("LAST\n"));
    assert!(
        repository.apply_hunk(&hunks[1]).is_err(),
        "original diff must become stale after staging"
    );
    let file = repository.load_state().unwrap().files[0].clone();
    let staged = repository.hunks(&file, true).unwrap();
    assert_eq!(staged.len(), 1);
    repository.apply_hunk(&staged[0]).unwrap();
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("[file] name")).unwrap(),
        lines("FIRST", "LAST")
    );
}

#[test]
fn hunk_without_final_newline_and_initial_staged_file_can_be_unstaged() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.write("file", "changed");
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    let hunk = repository.hunks(&file, false).unwrap().remove(0);
    repository.apply_hunk(&hunk).unwrap();
    assert_eq!(repo.git(&["show", ":file"]), "changed");
    let unborn = TestRepo::new();
    unborn.write("new", "new\n");
    unborn.git(&["add", "new"]);
    let file = unborn.open().load_state().unwrap().files[0].clone();
    let hunk = unborn.open().hunks(&file, true).unwrap().remove(0);
    unborn.open().apply_hunk(&hunk).unwrap();
    assert!(unborn.git(&["ls-files"]).is_empty());
    assert!(unborn.path.join("new").exists());
}

#[test]
fn stale_working_diff_and_structural_changes_are_not_partially_applied() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "changed\n");
    let repository = repo.open();
    let file = repository.load_state().unwrap().files[0].clone();
    let hunk = repository.hunks(&file, false).unwrap().remove(0);
    repo.write("file", "changed again\n");
    assert!(repository.apply_hunk(&hunk).is_err());
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    repo.git(&["mv", "file", "renamed"]);
    let file = repository.load_state().unwrap().files[0].clone();
    assert!(repository.hunks(&file, true).unwrap().is_empty());
}

#[test]
fn tui_selects_hunk_and_switches_between_index_and_working_diff() {
    let repo = TestRepo::new();
    repo.write("file", &lines("first", "last"));
    repo.commit_all("base");
    repo.write("file", &lines("FIRST", "LAST"));
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenHunks);
    assert_eq!(app.hunks.len(), 2);
    app.handle(Action::NextItem);
    app.handle(Action::Stage);
    assert!(repo.git(&["show", ":file"]).contains("LAST\n"));
    assert!(repo.git(&["show", ":file"]).contains("first\n"));
    app.handle(Action::ToggleComparison);
    assert!(app.staged_hunks && app.hunks.len() == 1);
    app.handle(Action::Unstage);
    assert!(repo.git(&["diff", "--cached"]).is_empty());
}

#[cfg(unix)]
#[test]
fn hunk_stage_supports_control_characters_in_unix_filenames() {
    {
        let repo = TestRepo::new();
        repo.write("line\nfile\t[1]", "base\n");
        repo.commit_all("base");
        repo.write("line\nfile\t[1]", "changed\n");
        let repository = repo.open();
        let file = repository.load_state().unwrap().files[0].clone();
        let hunk = repository.hunks(&file, false).unwrap().remove(0);
        repository.apply_hunk(&hunk).unwrap();
        assert_eq!(repo.git(&["show", ":line\nfile\t[1]"]), "changed\n");
    }
}

#[test]
fn upstream_review_never_stages_a_cached_hunk() {
    let repo = TestRepo::new();
    repo.write("file", "base\n");
    repo.commit_all("base");
    repo.write("file", "changed\n");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::OpenHunks);
    assert_eq!(app.hunks.len(), 1);
    app.upstream_comparison = true;
    app.handle(Action::Stage);
    assert!(repo.git(&["diff", "--cached"]).is_empty());
}
