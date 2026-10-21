//! Reproducible local probes; run through scripts/measure.py, never as CI thresholds.
use git_wirdo::keys::{KeyCode, KeyEvent, KeyModifiers};
use git_wirdo::{
    app::{Action, App},
    git::Repository,
    process::{Control, controlled, output},
};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
fn measure(name: &str, mut run: impl FnMut()) {
    run();
    let mut samples = Vec::new();
    for _ in 0..15 {
        let start = Instant::now();
        run();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"probe":name,"median_ms":samples[7],"p95_ms":samples[14]})
    );
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|s| s == "--hold") {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let repo = Repository::open(Path::new(&args[1])).unwrap();
    measure("startup", || {
        std::hint::black_box(App::new(repo.clone()).unwrap());
    });
    let mut app = App::new(repo.clone()).unwrap();
    app.set_history_window(3000, false);
    measure("refresh", || app.refresh().unwrap());
    let control = Arc::new(Control::default());
    measure("background_refresh", || {
        controlled(control.clone(), control.generation(), || {
            app.refresh().unwrap()
        })
    });
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
    let mut ui = git_wirdo::ui::Ui::default();
    measure("render", || {
        terminal.draw(|frame| ui.draw(frame, &app)).unwrap();
    });
    measure("open_diff", || {
        app.handle(Action::NextItem);
        assert!(!app.message_is_error());
    });
    let large = app
        .displayed_files()
        .iter()
        .position(|f| f.path == Path::new("zz-large-detail"))
        .unwrap();
    app.select(large);
    let rewind = |app: &mut App| {
        while app.detail_scroll() > 0 {
            app.handle(Action::PageUp);
        }
    };
    measure("page_down_100", || {
        rewind(&mut app);
        for _ in 0..100 {
            app.handle(Action::PageDown);
        }
    });
    app.handle(Action::Search);
    app.handle_paste("pending line 29999");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    measure("search", || {
        rewind(&mut app);
        app.handle(Action::NextMatch);
    });
    rewind(&mut app);
    measure("session_scroll_100", || {
        let mut session = git_wirdo::session::Session::new(app.clone());
        for _ in 0..100 {
            session.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        }
    });
    measure("cancel", || {
        let control = Arc::new(Control::default());
        let c = control.clone();
        let generation = control.generation();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            c.cancel();
        });
        let result = controlled(control, generation, || {
            output(std::process::Command::new(std::env::current_exe().unwrap()).arg("--hold"))
        });
        assert!(result.is_err());
        cancel.join().unwrap();
    });
}
