mod support;
use git_wirdo::app::{Action, App};
use git_wirdo::keys::{KeyCode, KeyEvent, KeyModifiers};
use support::TestRepo;

#[test]
fn individual_replacements_roundtrip_and_reject_stale_diff() {
    let repo = TestRepo::new();
    repo.write("file", "one\ntwo\nthree\nfour\n");
    repo.commit_all("base");
    repo.write("file", "ONE\ntwo\nTHREE\nfour\n");
    let git = repo.open();
    let file = git.load_state().unwrap().files.remove(0);
    let hunk = git.hunks(&file, false).unwrap().remove(0);
    git.apply_hunk(&hunk.select_lines(&[1, 2]).unwrap())
        .unwrap();
    assert_eq!(repo.git(&["show", ":file"]), "ONE\ntwo\nthree\nfour\n");
    assert!(
        git.apply_hunk(&hunk.select_lines(&[3, 4]).unwrap())
            .is_err()
    );
    let file = git.load_state().unwrap().files.remove(0);
    let hunk = git.hunks(&file, true).unwrap().remove(0);
    git.apply_hunk(&hunk.select_lines(&[1, 2]).unwrap())
        .unwrap();
    assert!(repo.git(&["diff", "--cached"]).is_empty());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("file")).unwrap(),
        "ONE\ntwo\nTHREE\nfour\n"
    );
}

#[test]
fn line_subsets_cover_additions_deletions_and_no_newline() {
    for (before, after, selected, expected) in [
        ("a\nb\n", "a\nx\ny\nb\n", vec![1], "a\nx\nb\n"),
        ("a\nx\ny\nb\n", "a\nb\n", vec![2], "a\nx\nb\n"),
        ("a\nb", "a\nB", vec![1, 2], "a\nB"),
        ("a\nb\n", "a\nB\n", vec![2], "a\nb\nB\n"),
    ] {
        let repo = TestRepo::new();
        repo.write("file", before);
        repo.commit_all("base");
        repo.write("file", after);
        let git = repo.open();
        let file = git.load_state().unwrap().files.remove(0);
        git.apply_hunk(
            &git.hunks(&file, false).unwrap()[0]
                .select_lines(&selected)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(repo.git(&["show", ":file"]), expected);
        let file = git.load_state().unwrap().files.remove(0);
        let hunk = git.hunks(&file, true).unwrap().remove(0);
        let all: Vec<_> = (1..=hunk
            .numbered_changes()
            .lines()
            .filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit()))
            .count())
            .collect();
        git.apply_hunk(&hunk.select_lines(&all).unwrap()).unwrap();
        assert!(repo.git(&["diff", "--cached"]).is_empty());
    }
}

#[test]
fn rebase_reorders_fixups_and_guards_todo_and_head() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.write("a", "a");
    repo.commit_all("a");
    repo.write("b", "b");
    repo.commit_all("b");
    let git = repo.open();
    let plan = git.prepare_interactive_rebase(&base).unwrap();
    assert!(
        git.run_interactive_rebase(&plan, "exec touch malicious")
            .is_err()
    );
    let lines: Vec<_> = plan.todo.lines().collect();
    let todo = format!("{}\n{}\n", lines[1], lines[0].replacen("pick", "fixup", 1));
    git.run_interactive_rebase(&plan, &todo).unwrap();
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "b");
    assert_eq!(std::fs::read_to_string(repo.path.join("a")).unwrap(), "a");
    assert!(
        !repo
            .git(&["for-each-ref", "refs/git-wirdo/recovery/"])
            .is_empty()
    );
    assert!(git.run_interactive_rebase(&plan, &plan.todo).is_err());
}

#[test]
fn filter_keeps_real_indices_and_empty_filter_cannot_stage_hidden_file() {
    let repo = TestRepo::new();
    repo.write("aaa", "a");
    repo.write("zzz", "z");
    let mut app = App::new(repo.open()).unwrap();
    app.handle(Action::FilterList);
    app.handle_paste("zzz");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.visible_indices(), vec![1]);
    assert_eq!(app.file_selection(), 1);
    app.handle(Action::Stage);
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]).trim(), "zzz");
    app.handle(Action::FilterList);
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_paste("missing");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle(Action::Stage);
    assert_eq!(repo.git(&["diff", "--cached", "--name-only"]).trim(), "zzz");
}

#[test]
fn rebase_conflict_can_abort_or_resolve_and_continue() {
    for abort in [true, false] {
        let repo = TestRepo::new();
        repo.write("file", "base\n");
        repo.commit_all("base");
        let base = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
        repo.write("file", "first\n");
        repo.commit_all("first");
        repo.write("file", "second\n");
        repo.commit_all("second");
        let head = repo.git(&["rev-parse", "HEAD"]);
        let git = repo.open();
        let plan = git.prepare_interactive_rebase(&base).unwrap();
        let lines: Vec<_> = plan.todo.lines().collect();
        assert!(
            git.run_interactive_rebase(&plan, &format!("{}\n{}\n", lines[1], lines[0]))
                .is_err()
        );
        assert!(git.load_state().unwrap().merge_state.in_progress());
        if abort {
            git.abort_operation().unwrap();
            assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
            assert_eq!(
                std::fs::read_to_string(repo.path.join("file")).unwrap(),
                "second\n"
            );
        } else {
            repo.write("file", "resolved\n");
            repo.git(&["add", "file"]);
            // The next reordered commit can conflict again; resolve each stop.
            for _ in 0..3 {
                if !git.load_state().unwrap().merge_state.in_progress() {
                    break;
                }
                let _ = git.continue_operation();
                if git.load_state().unwrap().merge_state.in_progress() {
                    repo.write("file", "final\n");
                    repo.git(&["add", "file"]);
                }
            }
            assert!(!git.load_state().unwrap().merge_state.in_progress());
        }
    }
}

#[test]
fn partial_unstage_uses_new_side_and_preserves_other_index_lines() {
    for (before, after, selected, expected) in [
        ("a\nb\n", "a\nx\ny\nb\n", vec![1], "a\ny\nb\n"),
        ("a\nx\ny\nb\n", "a\nb\n", vec![2], "a\ny\nb\n"),
        ("a\nb\n", "a\nB\n", vec![2], "a\n"),
        ("a\nb\n", "a\nB\n", vec![1], "a\nb\nB\n"),
        ("x\ny\n", "", vec![1], "x\n"),
    ] {
        let repo = TestRepo::new();
        repo.write("file", before);
        repo.commit_all("base");
        repo.write("file", after);
        repo.git(&["add", "file"]);
        let git = repo.open();
        let file = git.load_state().unwrap().files.remove(0);
        git.apply_hunk(
            &git.hunks(&file, true).unwrap()[0]
                .select_lines(&selected)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(repo.git(&["show", ":file"]), expected);
        assert_eq!(
            std::fs::read_to_string(repo.path.join("file")).unwrap(),
            after
        );
    }
}

#[test]
fn rebase_refuses_dirty_checkout_missing_duplicates_and_detached_head() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.write("a", "a");
    repo.commit_all("a");
    repo.write("b", "b");
    repo.commit_all("b");
    let git = repo.open();
    let plan = git.prepare_interactive_rebase(&base).unwrap();
    let head = repo.git(&["rev-parse", "HEAD"]);
    let first = plan.todo.lines().next().unwrap();
    for todo in [
        first.to_owned(),
        format!("{first}\n{first}"),
        plan.todo.replacen("pick", "squash", 1),
    ] {
        assert!(git.run_interactive_rebase(&plan, &todo).is_err());
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    }
    repo.write("untracked", "keep");
    assert!(git.run_interactive_rebase(&plan, &plan.todo).is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path.join("untracked")).unwrap(),
        "keep"
    );
    std::fs::remove_file(repo.path.join("untracked")).unwrap();
    repo.git(&["checkout", "--detach"]);
    assert!(git.prepare_interactive_rebase(&base).is_err());
}

#[test]
fn squash_retains_combined_message_and_final_tree() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.write("a", "a");
    repo.commit_all("first subject");
    repo.write("b", "b");
    repo.commit_all("second subject");
    let tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
    let git = repo.open();
    let plan = git.prepare_interactive_rebase(&base).unwrap();
    let rows: Vec<_> = plan.todo.lines().collect();
    git.run_interactive_rebase(
        &plan,
        &format!("{};{}", rows[0], rows[1].replacen("pick", "squash", 1)),
    )
    .unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD^{tree}"]), tree);
    let message = repo.git(&["log", "-1", "--format=%B"]);
    assert!(message.contains("first subject") && message.contains("second subject"));
}

#[test]
fn branch_name_filter_is_unicode_aware_and_keeps_identity_on_refresh() {
    let repo = TestRepo::new();
    repo.write("file", "base");
    repo.commit_all("base");
    repo.git(&["branch", "Ä-topic"]);
    repo.git(&["branch", "zzz"]);
    let mut app = App::new(repo.open()).unwrap();
    app.set_view(git_wirdo::model::ViewMode::Branches);
    app.handle(Action::FilterList);
    app.handle_paste("ä-TOP");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.visible_indices().len(), 1);
    repo.git(&["branch", "aaa"]);
    app.handle(Action::Refresh);
    assert_eq!(app.state().branches[app.branch_selection()].name, "Ä-topic");
    app.handle(Action::SwitchBranch);
    assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "Ä-topic");
}

#[test]
fn reword_preserves_message_bytes_and_treats_shell_syntax_as_data() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write("a", "a");
    repo.commit_all("a\n\noriginal multiline body");
    let sha = repo.git(&["rev-parse", "HEAD"]);
    let git = repo.open();
    let mut plan = git.prepare_interactive_rebase(base.trim()).unwrap();
    assert!(
        plan.message(sha.trim())
            .unwrap()
            .contains("original multiline body")
    );
    let message = "Новое имя\n\n$() `touch injected` ' \" ; # data\\n\n";
    plan.reword(sha.trim(), message).unwrap();
    git.run_interactive_rebase(&plan, &plan.todo.replace("pick ", "reword "))
        .unwrap();
    assert_eq!(
        repo.git(&["log", "-1", "--format=%B"]).trim_end(),
        message.trim_end()
    );
    assert!(!repo.path.join("injected").exists());
    assert_eq!(repo.git(&["show", "HEAD:a"]), "a");
    assert!(
        repo.git(&[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/git-wirdo/recovery/"
        ])
        .contains(sha.trim())
    );
}

#[test]
fn reword_survives_an_edit_stop_and_drop_is_explicit_and_recoverable() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]);
    for name in ["a", "b", "c"] {
        repo.write(name, name);
        repo.commit_all(name);
    }
    let original = repo.git(&["rev-parse", "HEAD"]);
    let git = repo.open();
    let mut plan = git.prepare_interactive_rebase(base.trim()).unwrap();
    let rows: Vec<_> = plan.todo.lines().collect();
    let third = rows[2].split_whitespace().nth(1).unwrap().to_owned();
    let todo = format!(
        "{}\n{}\n{}",
        rows[0].replacen("pick", "edit", 1),
        rows[1].replacen("pick", "drop", 1),
        rows[2].replacen("pick", "reword", 1)
    );
    plan.reword(&third, "message after restart\n\nbody\n")
        .unwrap();
    git.run_interactive_rebase(&plan, &todo).unwrap();
    assert!(git.load_state().unwrap().merge_state.rebase_in_progress);
    // A fresh Git process (without the initial editor environment) must retain the message.
    repo.git(&["rebase", "--continue"]);
    assert!(!git.load_state().unwrap().merge_state.rebase_in_progress);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]).trim(),
        "message after restart"
    );
    assert!(!repo.path.join("b").exists());
    assert!(repo.path.join("a").exists() && repo.path.join("c").exists());
    assert_eq!(
        repo.git(&["rev-list", "--count", &format!("{}..HEAD", base.trim())])
            .trim(),
        "2"
    );
    assert!(
        repo.git(&[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/git-wirdo/recovery/"
        ])
        .contains(original.trim())
    );
}

#[test]
fn rebase_rejects_all_drop_and_reword_without_a_message_before_changing_refs() {
    let repo = TestRepo::new();
    repo.write("base", "base");
    repo.commit_all("base");
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.write("a", "a");
    repo.commit_all("a");
    let git = repo.open();
    let plan = git.prepare_interactive_rebase(base.trim()).unwrap();
    let before = repo.git(&["show-ref"]);
    for action in ["drop", "reword", "exec"] {
        assert!(
            git.run_interactive_rebase(&plan, &plan.todo.replacen("pick", action, 1))
                .is_err()
        );
        assert_eq!(repo.git(&["show-ref"]), before);
    }
}
