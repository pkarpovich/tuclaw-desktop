use gpui::{App, AppContext, KeyBinding, Menu, MenuItem, PromptLevel, SystemMenuType, actions};

actions!(tuclaw, [About, Quit]);

pub fn version_line() -> String {
    format!("{} ({})", env!("CARGO_PKG_VERSION"), env!("TUCLAW_COMMIT"))
}

pub fn install(cx: &mut App) {
    cx.on_action(about);
    cx.on_action(quit);
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus([Menu::new("Tuclaw").items([
        MenuItem::action("About Tuclaw", About),
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action("Quit Tuclaw", Quit),
    ])]);
}

fn about(_: &About, cx: &mut App) {
    cx.defer(show_version);
}

fn show_version(cx: &mut App) {
    let Some(window) = cx.active_window() else {
        return;
    };
    let detail = version_line();
    let shown = window.update(cx, |_, window, cx| {
        window.prompt(PromptLevel::Info, "Tuclaw", Some(&detail), &["OK"], cx)
    });
    let Ok(answer) = shown else {
        return;
    };
    cx.background_spawn(async move {
        answer.await.ok();
    })
    .detach();
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

#[cfg(test)]
mod tests {
    use gpui::{Empty, TestAppContext};

    use super::*;

    #[test]
    fn the_version_line_names_the_crate_version_and_the_commit() {
        let line = version_line();
        assert!(line.starts_with(env!("CARGO_PKG_VERSION")), "{line}");
        assert!(
            line.ends_with(&format!("({})", env!("TUCLAW_COMMIT"))),
            "{line}"
        );
    }

    #[gpui::test]
    fn about_shows_the_version_in_a_prompt(cx: &mut TestAppContext) {
        cx.update(install);
        let window = cx.add_window(|_, _| Empty);
        window
            .update(cx, |_, window, _| window.activate_window())
            .expect("the window is open");
        cx.update(|cx| cx.dispatch_action(&About));
        cx.run_until_parked();
        assert_eq!(
            cx.pending_prompt(),
            Some(("Tuclaw".to_string(), version_line()))
        );
        cx.simulate_prompt_answer("OK");
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
    }

    #[gpui::test]
    fn about_without_a_window_does_nothing(cx: &mut TestAppContext) {
        cx.update(install);
        cx.update(|cx| cx.dispatch_action(&About));
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
    }

    #[gpui::test]
    fn quit_is_bound_to_cmd_q(cx: &mut TestAppContext) {
        cx.update(install);
        let bound =
            cx.update(|cx| cx.all_bindings_for_input(&[gpui::Keystroke::parse("cmd-q").unwrap()]));
        let mut quits = 0;
        for binding in bound {
            if binding.action().partial_eq(&Quit) {
                quits += 1;
            }
        }
        assert_eq!(quits, 1);
    }
}
