use gpui::{App, AppContext, Context, IntoElement, Render, Window, WindowOptions, div};

struct Root;

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Root))
            .expect("failed to open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::Root;
    use gpui::{AppContext, TestAppContext};

    #[gpui::test]
    fn root_entity_is_constructible(cx: &mut TestAppContext) {
        let root = cx.new(|_| Root);
        root.read_with(cx, |_root, _cx| {});
    }
}
