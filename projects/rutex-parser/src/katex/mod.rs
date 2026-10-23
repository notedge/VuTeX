use gloo_utils::document;
pub use katex_wasmbind::KaTeXOptions;
use yew::{prelude::*, Component, Html};

#[derive(Properties, Clone, PartialEq)]
pub struct KaTeXProperties {
    pub math: String,
    #[prop_or(true)]
    pub inline: bool,
    #[prop_or(String::from("html"))]
    pub output: String,
}

pub struct KaTeX;

impl Component for KaTeX {
    type Message = ();
    type Properties = KaTeXProperties;

    fn create(_ctx: &yew::Context<Self>) -> Self {
        Self
    }

    fn view(&self, ctx: &yew::Context<Self>) -> Html {
        let renderer = self.create_renderer(ctx.props());
        let t = document().create_element("div").unwrap();
        t.set_inner_html(&renderer.render(ctx.props().math.as_str()));
        Html::VRef(t.first_child().unwrap())
    }

    #[cfg(feature = "auto-cdn")]
    fn rendered(&mut self, _ctx: &yew::Context<Self>, first_render: bool) {
        if first_render {
            self.load_cdn().unwrap_or_default()
        }
    }
}

impl KaTeX {
    pub fn create_renderer(&self, props: &KaTeXProperties) -> KaTeXOptions {
        let mut render = if props.inline { KaTeXOptions::inline_mode() } else { KaTeXOptions::display_mode() };
        render.set_output_format(props.output.as_str());
        render
    }

    pub fn load_cdn(&self) -> Result<(), std::io::Error> {
        // <link rel="stylesheet" href="https://unpkg.com/katex@0.12.0/dist/katex.min.css">
        if document().get_element_by_id("cdn-katex").is_none() {
            let head = document().query_selector("head").expect("").expect("");
            let t = document().create_element("link").expect("");
            // async css load
            t.set_attribute("id", "cdn-katex").expect("");
            t.set_attribute("media", "none").expect("");
            t.set_attribute("onload", "this.media='all'").expect("");
            t.set_attribute("rel", "stylesheet").expect("");
            t.set_attribute("href", "https://unpkg.com/katex@0.12.0/dist/katex.min.css").expect("");
            head.append_child(&t).expect("");
        }
        Ok(())
    }
}
