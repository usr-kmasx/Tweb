use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, EventControllerKey};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use vte::{Pty, PtyFlags, Terminal, TerminalExt};
use webkit::prelude::WebViewExt;
use webkit::{
    NetworkSession, UserContentInjectedFrames, UserContentManager, UserScript,
    UserScriptInjectionTime, WebView,
};

const APP_ID: &str = "dev.tweb.Tweb";
const DDG_URL: &str = "https://duckduckgo.com";
const SOCK_NAME: &str = "tweb-web.sock";
const TAB_W: i32 = 162;
const TAB_H: i32 = 26;

const TABS_CSS: &str = "
.tweb-tab {
    border-radius: 0;
    border: none;
    box-shadow: none;
    outline: none;
    padding: 0 8px;
}
.tweb-tab-label {
    color: #FFFFFF;
}
.tweb-tab-active {
    background: #FFFFFF;
}
.tweb-tab-active .tweb-tab-label {
    color: #000000;
}
.tweb-tab-inactive {
    background: #595959;
}
.tweb-tabs-scroll scrollbar.horizontal slider {
    min-height: 4px;
}
";

const CLEAN_FULLSCREEN_JS: &str = r#"(function() {
  function vids() { return Array.prototype.slice.call(document.querySelectorAll('video')); }
  function area(v) {
    var r = v.getBoundingClientRect();
    return Math.max(0, r.width) * Math.max(0, r.height);
  }
  function playing(v) { return !v.paused && !v.ended && v.readyState > 2; }
  function biggest(list) {
    list = list.filter(function(v) { return area(v) > 0; });
    list.sort(function(a, b) { return ((playing(b) ? 1 : 0) - (playing(a) ? 1 : 0)) || (area(b) - area(a)); });
    return list[0] || null;
  }
  function ensureControls(v) {
    if (v && !v.hasAttribute('controls')) { v.setAttribute('controls', ''); v.setAttribute('data-tweb-controls', '1'); }
  }
  function cleanupControls() {
    Array.prototype.forEach.call(document.querySelectorAll('video[data-tweb-controls]'), function(v) {
      v.removeAttribute('controls');
      v.removeAttribute('data-tweb-controls');
    });
  }
  var obs = new MutationObserver(function(muts) {
    if (!document.fullscreenElement) return;
    muts.forEach(function(m) {
      var v = m.target;
      if (v.tagName === 'VIDEO' && !v.hasAttribute('controls')) { ensureControls(v); }
    });
  });
  obs.observe(document, { attributes: true, attributeFilter: ['controls'], subtree: true });
  document.addEventListener('fullscreenchange', function() {
    if (document.fullscreenElement) {
      var fs = document.fullscreenElement;
      var v = (fs.tagName === 'VIDEO') ? fs : biggest(Array.prototype.slice.call(fs.querySelectorAll('video')));
      if (v) { ensureControls(v); }
    } else {
      cleanupControls();
    }
  }, true);
  function injectCss() {
    if (document.querySelector('style[data-tweb]')) return;
    var css = ':fullscreen .ytp-chrome-top,:fullscreen .ytp-chrome-bottom,' +
      ':fullscreen .ytp-gradient-top,:fullscreen .ytp-gradient-bottom,' +
      ':fullscreen .ytp-chrome-controls,:fullscreen .ytp-settings-menu,' +
      ':fullscreen .ytp-popup{display:none !important;}';
    var st = document.createElement('style');
    st.setAttribute('data-tweb', '1');
    st.textContent = css;
    (document.head || document.documentElement).appendChild(st);
  }
  if (document.head || document.documentElement) { injectCss(); }
  else { document.addEventListener('DOMContentLoaded', injectCss, { once: true }); }
  document.addEventListener('keydown', function(e) {
    if (e.key !== 'f' && e.key !== 'F') return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    var ae = document.activeElement;
    if (ae && (ae.tagName === 'INPUT' || ae.tagName === 'TEXTAREA' || ae.isContentEditable)) return;
    e.stopPropagation();
    e.preventDefault();
    var v = biggest(vids());
    if (!v) return;
    ensureControls(v);
    var p = v.requestFullscreen ? v.requestFullscreen() : null;
    if (p && p.catch) { p.catch(function() {}); }
  }, true);
})();"#;

struct Tab {
    view: WebView,
    button: gtk::Button,
}

struct WebUI {
    overlay: gtk::Overlay,
    stack: gtk::Stack,
    scroll: gtk::ScrolledWindow,
    bar: gtk::Box,
}

struct AppState {
    window: ApplicationWindow,
    terminal: Terminal,
    web_ui: RefCell<Option<WebUI>>,
    tabs: RefCell<Vec<Tab>>,
    current: Cell<usize>,
    in_web: RefCell<bool>,
}

fn web_shim_main() -> glib::ExitCode {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    match UnixStream::connect(socket_path()) {
        Ok(mut s) => {
            s.set_write_timeout(Some(std::time::Duration::from_secs(2)))
                .ok();
            match s.write_all(b"web\n") {
                Ok(()) => glib::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("web: nao foi possivel falar com o Tweb ({e})");
                    glib::ExitCode::FAILURE
                }
            }
        }
        Err(e) => {
            eprintln!("web: nao foi possivel falar com o Tweb ({e})");
            glib::ExitCode::FAILURE
        }
    }
}

fn invoked_as_web() -> bool {
    std::env::args()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .map(|n| n == "web")
        .unwrap_or(false)
}

fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

fn socket_path() -> std::path::PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
    std::path::Path::new(&dir).join(SOCK_NAME)
}

fn shim_dir() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("bin");
            if cand.join("web").is_file() {
                return cand.to_str().map(|s| s.to_string());
            }
        }
    }
    if let Some(dir) = option_env!("CARGO_MANIFEST_DIR") {
        let cand = std::path::Path::new(dir).join("bin");
        if cand.join("web").is_file() {
            return cand.to_str().map(|s| s.to_string());
        }
    }
    None
}

fn child_env() -> Vec<String> {
    let mut seen_path = false;
    let mut env: Vec<String> = std::env::vars()
        .map(|(k, v)| {
            if k == "PATH" {
                seen_path = true;
                match shim_dir() {
                    Some(d) => format!("PATH={d}:{v}"),
                    None => format!("PATH={v}"),
                }
            } else {
                format!("{k}={v}")
            }
        })
        .collect();
    if !seen_path {
        if let Some(d) = shim_dir() {
            env.push(format!("PATH={d}:/usr/bin:/bin"));
        }
    }
    env
}

fn spawn_shell(terminal: &Terminal, pty: &Pty) {
    let shell = user_shell();
    let home = std::env::var("HOME").ok();
    let env_owned = child_env();
    let env: Vec<&str> = env_owned.iter().map(|s| s.as_str()).collect();

    terminal.set_pty(Some(pty));

    pty.spawn_async(
        home.as_deref(),
        &[shell.as_str()],
        &env,
        glib::SpawnFlags::empty(),
        || {},
        -1,
        None::<&gio::Cancellable>,
        |result| {
            if let Err(e) = result {
                eprintln!("tweb: falha ao iniciar shell: {e}");
            }
        },
    );
}

fn apply_tabs_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(TABS_CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn refresh_tab_colors(state: &AppState) {
    let cur = state.current.get();
    for (i, tab) in state.tabs.borrow().iter().enumerate() {
        if i == cur {
            tab.button.add_css_class("tweb-tab-active");
            tab.button.remove_css_class("tweb-tab-inactive");
        } else {
            tab.button.add_css_class("tweb-tab-inactive");
            tab.button.remove_css_class("tweb-tab-active");
        }
    }
}

fn reveal_tab(state: &Rc<AppState>, index: usize) {
    let s = state.clone();
    glib::idle_add_local_once(move || {
        if index >= s.tabs.borrow().len() {
            return;
        }
        if let Some(ui) = s.web_ui.borrow().as_ref() {
            let adj = ui.scroll.hadjustment();
            let x = index as f64 * (TAB_W + 8) as f64;
            let w = TAB_W as f64;
            let val = adj.value();
            let page = adj.page_size();
            if x < val {
                adj.set_value(x);
            } else if x + w > val + page {
                adj.set_value(x + w - page);
            }
        }
    });
}

fn switch_tab(state: &Rc<AppState>, index: usize) {
    let tabs = state.tabs.borrow();
    if tabs.is_empty() {
        return;
    }
    let index = index.min(tabs.len() - 1);
    drop(tabs);
    state.current.set(index);
    if let (Some(ui), Some(tab)) = (
        state.web_ui.borrow().as_ref(),
        state.tabs.borrow().get(index),
    ) {
        ui.stack.set_visible_child(&tab.view);
        refresh_tab_colors(state);
        tab.view.grab_focus();
    }
    reveal_tab(state, index);
}

fn show_terminal(state: &AppState) {
    state.window.set_child(Some(&state.terminal));
    state.terminal.grab_focus();
    *state.in_web.borrow_mut() = false;
}

fn ensure_web_ui(state: &Rc<AppState>) -> gtk::Overlay {
    if let Some(ui) = state.web_ui.borrow().as_ref() {
        return ui.overlay.clone();
    }
    let stack = gtk::Stack::new();
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.set_halign(gtk::Align::Start);
    bar.set_valign(gtk::Align::Center);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scroll.add_css_class("tweb-tabs-scroll");
    scroll.set_propagate_natural_width(true);
    scroll.set_hexpand(true);
    scroll.set_halign(gtk::Align::Fill);
    scroll.set_valign(gtk::Align::End);
    scroll.set_margin_start(8);
    scroll.set_margin_end(8);
    scroll.set_margin_bottom(8);
    scroll.set_child(Some(&bar));
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&stack));
    overlay.add_overlay(&scroll);
    let ov = overlay.clone();
    state.web_ui.borrow_mut().replace(WebUI {
        overlay,
        stack,
        scroll,
        bar,
    });
    ov
}

fn open_tab(state: &Rc<AppState>, url: &str) {
    let overlay = ensure_web_ui(state);
    let session = NetworkSession::new_ephemeral();
    let ucm = UserContentManager::new();
    ucm.add_script(&UserScript::new(
        CLEAN_FULLSCREEN_JS,
        UserContentInjectedFrames::AllFrames,
        UserScriptInjectionTime::Start,
        &[],
        &[],
    ));
    let view = WebView::builder()
        .network_session(&session)
        .user_content_manager(&ucm)
        .build();

    let label = gtk::Label::new(Some("Nova aba"));
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(16);
    label.add_css_class("tweb-tab-label");
    let button = gtk::Button::new();
    button.set_size_request(TAB_W, TAB_H);
    button.set_child(Some(&label));
    button.add_css_class("tweb-tab");
    button.add_css_class("tweb-tab-inactive");

    let label_title = label.clone();
    view.connect_title_notify(move |v| {
        let title = v.title().map(|t| t.to_string()).unwrap_or_default();
        if title.trim().is_empty() {
            label_title.set_text("Nova aba");
        } else {
            label_title.set_text(&title);
        }
    });

    let opener = state.clone();
    view.connect_create(move |_, action| {
        if let Some(req) = action.request() {
            if let Some(uri) = req.uri() {
                open_tab(&opener, &uri);
            }
        }
        None
    });

    {
        let ui = state.web_ui.borrow();
        let ui = ui.as_ref().expect("web ui criada acima");
        ui.stack.add_child(&view);
        ui.bar.append(&button);
    }

    let index = state.tabs.borrow().len();
    let switcher = state.clone();
    let btn = button.clone();
    button.connect_clicked(move |_| {
        let pos = switcher
            .tabs
            .borrow()
            .iter()
            .position(|t| t.button == btn);
        if let Some(i) = pos {
            switch_tab(&switcher, i);
        }
    });
    state.tabs.borrow_mut().push(Tab {
        view: view.clone(),
        button,
    });

    state.window.set_child(Some(&overlay));
    *state.in_web.borrow_mut() = true;
    view.load_uri(url);
    switch_tab(state, index);
}

fn close_tab(state: &Rc<AppState>, index: usize) {
    if state.tabs.borrow().is_empty() || index >= state.tabs.borrow().len() {
        return;
    }
    let was_current = state.current.get() == index;
    let cur = state.current.get();
    {
        let mut tabs = state.tabs.borrow_mut();
        let tab = tabs.remove(index);
        if let Some(ui) = state.web_ui.borrow().as_ref() {
            ui.stack.remove(&tab.view);
            ui.bar.remove(&tab.button);
        }
    }
    if state.tabs.borrow().is_empty() {
        show_terminal(state);
    } else if was_current {
        switch_tab(state, index.min(state.tabs.borrow().len() - 1));
    } else {
        let fixed = if index < cur { cur - 1 } else { cur };
        state.current.set(fixed.min(state.tabs.borrow().len() - 1));
    }
}

fn watch_web_command(tx: std::sync::mpsc::Sender<()>) {
    use std::io::Read;
    use std::os::unix::net::UnixListener;

    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("tweb: socket {path:?}: {e}");
            return;
        }
    };
    for conn in listener.incoming() {
        if let Ok(mut stream) = conn {
            let mut buf = [0u8; 64];
            let msg = match stream.read(&mut buf) {
                Ok(n) => String::from_utf8_lossy(&buf[..n]).trim().to_string(),
                Err(_) => continue,
            };
            if msg == "web" {
                let _ = tx.send(());
            }
        }
    }
}

fn build_ui(app: &Application) {
    apply_tabs_css();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Tweb")
        .default_width(900)
        .default_height(600)
        .build();

    let terminal = Terminal::new();

    let pty = Pty::new_sync(PtyFlags::DEFAULT, None::<&gio::Cancellable>)
        .expect("tweb: falha ao criar PTY");
    spawn_shell(&terminal, &pty);

    let state = Rc::new(AppState {
        window: window.clone(),
        terminal: terminal.clone(),
        web_ui: RefCell::new(None),
        tabs: RefCell::new(Vec::new()),
        current: Cell::new(0),
        in_web: RefCell::new(false),
    });

    let app_weak = app.downgrade();
    terminal.connect_child_exited(move |_, _| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
    });

    let keys_state = state.clone();
    let keys = EventControllerKey::new();    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if ctrl && shift && (key == gdk::Key::T || key == gdk::Key::t) {
            show_terminal(&keys_state);
            return glib::Propagation::Stop;
        }
        if !*keys_state.in_web.borrow() {
            return glib::Propagation::Proceed;
        }
        if ctrl && (key == gdk::Key::W || key == gdk::Key::w) {
            close_tab(&keys_state, keys_state.current.get());
            return glib::Propagation::Stop;
        }
        if ctrl && !shift && (key == gdk::Key::t || key == gdk::Key::T) {
            open_tab(&keys_state, DDG_URL);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    let arrows = state.clone();
    let arrow_ctl = EventControllerKey::new();
    arrow_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    arrow_ctl.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if ctrl && shift && *arrows.in_web.borrow() {
            let n = arrows.tabs.borrow().len();
            if n > 0 {
                if key == gdk::Key::Left {
                    let cur = (arrows.current.get() + n - 1) % n;
                    switch_tab(&arrows, cur);
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Right {
                    let cur = (arrows.current.get() + 1) % n;
                    switch_tab(&arrows, cur);
                    return glib::Propagation::Stop;
                }
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(arrow_ctl);

    let term_keys = state.clone();
    let term_ctl = EventControllerKey::new();
    term_ctl.set_propagation_phase(gtk::PropagationPhase::Capture);
    term_ctl.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if ctrl
            && shift
            && (key == gdk::Key::T || key == gdk::Key::t)
            && !term_keys.tabs.borrow().is_empty()
        {
            if let Some(ui) = term_keys.web_ui.borrow().as_ref() {
                term_keys.window.set_child(Some(&ui.overlay));
                *term_keys.in_web.borrow_mut() = true;
            }
            switch_tab(&term_keys, term_keys.current.get());
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    terminal.add_controller(term_ctl);

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("tweb-web-ipc".to_string())
        .spawn(move || watch_web_command(tx))
        .expect("tweb: falha ao criar thread de IPC");
    let opener = state.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
        while rx.try_recv().is_ok() {
            open_tab(&opener, DDG_URL);
        }
        glib::ControlFlow::Continue
    });

    window.set_child(Some(&terminal));
    window.present();
    terminal.grab_focus();
}

fn main() -> glib::ExitCode {
    if invoked_as_web() {
        return web_shim_main();
    }
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
