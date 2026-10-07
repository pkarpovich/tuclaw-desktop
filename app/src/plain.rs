use markdown::mdast::Node;
use markdown::{ParseOptions, to_mdast};

pub fn plain_text(text: &str) -> String {
    let mut options = ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(root) = to_mdast(text, &options) else {
        return collapse(text);
    };
    let mut out = String::new();
    write(&root, &mut out);
    collapse(&out)
}

fn write(node: &Node, out: &mut String) {
    match node {
        Node::Text(text) => out.push_str(&text.value),
        Node::InlineCode(code) => out.push_str(&code.value),
        Node::InlineMath(math) => out.push_str(&math.value),
        Node::Code(code) => block(&code.value, out),
        Node::Math(math) => block(&math.value, out),
        Node::Image(image) => out.push_str(&image.alt),
        Node::ImageReference(image) => out.push_str(&image.alt),
        Node::Break(_) => out.push(' '),
        Node::Delete(_) => children(node, out),
        Node::Emphasis(_) => children(node, out),
        Node::Strong(_) => children(node, out),
        Node::Link(_) => children(node, out),
        Node::LinkReference(_) => children(node, out),
        Node::Root(_) => blocks(node, out),
        Node::Blockquote(_) => blocks(node, out),
        Node::List(_) => blocks(node, out),
        Node::ListItem(_) => blocks(node, out),
        Node::Heading(_) => blocks(node, out),
        Node::Paragraph(_) => blocks(node, out),
        Node::Table(_) => blocks(node, out),
        Node::TableRow(_) => blocks(node, out),
        Node::TableCell(_) => blocks(node, out),
        Node::FootnoteDefinition(_) => {}
        Node::FootnoteReference(_) => {}
        Node::Definition(_) => {}
        Node::ThematicBreak(_) => {}
        Node::Html(_) => {}
        Node::Toml(_) => {}
        Node::Yaml(_) => {}
        Node::MdxjsEsm(_) => {}
        Node::MdxJsxFlowElement(_) => {}
        Node::MdxJsxTextElement(_) => {}
        Node::MdxFlowExpression(_) => {}
        Node::MdxTextExpression(_) => {}
    }
}

fn children(node: &Node, out: &mut String) {
    let Some(children) = node.children() else {
        return;
    };
    for child in children {
        write(child, out);
    }
}

fn blocks(node: &Node, out: &mut String) {
    out.push(' ');
    children(node, out);
    out.push(' ');
}

fn block(text: &str, out: &mut String) {
    out.push(' ');
    out.push_str(text);
    out.push(' ');
}

fn collapse(text: &str) -> String {
    let mut words = Vec::new();
    for word in text.split_whitespace() {
        words.push(word);
    }
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::plain_text;

    #[test]
    fn emphasis_and_links_lose_their_markup() {
        assert_eq!(
            plain_text(
                "Пора брать билет на **Verity, 21:50, Sala 5** ([купить](https://tickets.example.test/api/order/1)). Советую _ряд_ ~~7~~ 8."
            ),
            "Пора брать билет на Verity, 21:50, Sala 5 (купить). Советую ряд 7 8."
        );
    }

    #[test]
    fn inline_code_keeps_its_content() {
        assert_eq!(
            plain_text("Run `mise run test` now"),
            "Run mise run test now"
        );
    }

    #[test]
    fn blocks_are_joined_with_spaces() {
        let text = "# Summary\n\n> quoted line\n\n- first\n- second\n\n1. one\n2. two";
        assert_eq!(plain_text(text), "Summary quoted line first second one two");
    }

    #[test]
    fn tables_and_fenced_code_collapse_to_their_text() {
        let text = "| Film | Time |\n|---|---|\n| Verity | 21:50 |\n\n```sh\nls -la\n```";
        assert_eq!(plain_text(text), "Film Time Verity 21:50 ls -la");
    }

    #[test]
    fn an_image_becomes_its_alt_text() {
        assert_eq!(
            plain_text("Look: ![a turtle](https://media.example.test/t.png)"),
            "Look: a turtle"
        );
    }
}
