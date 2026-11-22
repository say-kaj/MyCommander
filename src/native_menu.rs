//! The real macOS menu bar at the top of the screen, with Midnight Commander's
//! structure: myCommander (app menu), Left, File, Command, Options, Right, Help.
//! Items come from `menus.rs`, so the in-window menu bar and this one match.
//! Shortcuts given here are Cmd-based ones that no text field needs.

use crate::app::Action;

#[cfg(target_os = "macos")]
pub struct NativeMenu {
    _menu: muda::Menu,
    ids: Vec<(String, Action)>,
}

#[cfg(target_os = "macos")]
fn accel_for(a: Action) -> Option<muda::accelerator::Accelerator> {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    let cmd = |c: Code| Some(Accelerator::new(Some(Modifiers::SUPER), c));
    let cmd_shift = |c: Code| Some(Accelerator::new(Some(Modifiers::SUPER | Modifiers::SHIFT), c));
    match a {
        Action::Config => cmd(Code::Comma),
        Action::Quit => cmd(Code::KeyQ),
        Action::Find => cmd_shift(Code::KeyF),
        Action::Hotlist => cmd(Code::KeyD),
        Action::Mkdir => cmd_shift(Code::KeyN),
        Action::Rescan(0) => cmd(Code::KeyR),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
impl NativeMenu {
    pub fn install() -> Option<NativeMenu> {
        use muda::{AboutMetadata, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

        let mut ids: Vec<(String, Action)> = Vec::new();
        let mut counter = 0;
        let mut make = |label: &str, action: Action, enabled: bool| -> MenuItem {
            counter += 1;
            let id = format!("mc-{}", counter);
            ids.push((id.clone(), action));
            MenuItem::with_id(id, label, enabled, accel_for(action))
        };

        let about = PredefinedMenuItem::about(
            Some("About myCommander"),
            Some(AboutMetadata {
                name: Some("myCommander".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                authors: Some(vec!["Kay Mühlena".into()]),
                copyright: Some("© Kay Mühlena".into()),
                // macOS shows only `credits` in the About panel (no website field there).
                credits: Some("https://github.com/say-kaj/MyCommander".into()),
                website: Some("https://github.com/say-kaj/MyCommander".into()),
                website_label: Some("github.com/say-kaj/MyCommander".into()),
                comments: Some("A two-panel file manager for macOS, organised like Midnight Commander.".into()),
                ..Default::default()
            }),
        );
        let settings = make("Settings…", Action::Config, true);
        let save = make("Save setup", Action::SaveSetup, true);
        let quit = make("Quit myCommander", Action::Quit, true);
        let app_menu = Submenu::with_items(
            "myCommander",
            true,
            &[
                &about,
                &PredefinedMenuItem::separator(),
                &settings,
                &save,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &quit,
            ],
        )
        .ok()?;

        let menu = Menu::new();
        menu.append(&app_menu).ok()?;
        for (idx, title) in crate::menus::TITLES.iter().enumerate() {
            let sub = Submenu::new(*title, true);
            for it in crate::menus::items(idx) {
                match it.action {
                    None => {
                        sub.append(&PredefinedMenuItem::separator()).ok()?;
                    }
                    Some(a) => {
                        let label = if it.key.is_empty() || accel_for(a).is_some() { it.label.to_string() } else { format!("{}\t{}", it.label, it.key) };
                        let mi = make(&label, a, it.enabled);
                        sub.append(&mi as &dyn IsMenuItem).ok()?;
                    }
                }
            }
            menu.append(&sub).ok()?;
        }
        let help = make("myCommander Help\tF1", Action::Help, true);
        let check = make("Check for Updates…", Action::CheckUpdates, true);
        let github = make("myCommander on GitHub…", Action::OpenGithub, true);
        let help_menu = Submenu::with_items("Help", true, &[&help, &PredefinedMenuItem::separator(), &check, &github]).ok()?;
        menu.append(&help_menu).ok()?;
        menu.init_for_nsapp();
        Some(NativeMenu { _menu: menu, ids })
    }

    /// Actions chosen from the menu bar since the last frame.
    pub fn poll(&self) -> Vec<Action> {
        let mut out = Vec::new();
        while let Ok(ev) = muda::MenuEvent::receiver().try_recv() {
            let id: &str = ev.id.as_ref();
            if let Some((_, a)) = self.ids.iter().find(|(i, _)| i == id) {
                out.push(*a);
            }
        }
        out
    }
}

#[cfg(not(target_os = "macos"))]
pub struct NativeMenu;

#[cfg(not(target_os = "macos"))]
impl NativeMenu {
    pub fn install() -> Option<NativeMenu> {
        None
    }
    pub fn poll(&self) -> Vec<Action> {
        Vec::new()
    }
}
