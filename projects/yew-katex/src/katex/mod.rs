pub use katex_wasmbind::{KaTeXOptions};
use yew::{prelude::*, Component, ComponentLink, Html, ShouldRender};
use yew::utils::document;

#[derive(Properties, Clone, PartialEq)]
pub struct KaTeXProperties {
    pub math: String,
    #[prop_or(true)]
    pub inline: bool,
    #[prop_or(String::from("html"))]
    pub output: String,
}

pub struct KaTeX {
    pub math_rendered: Html,
    pub props: KaTeXProperties,
}

impl Component for KaTeX {
    type Message = ();
    type Properties = KaTeXProperties;

    fn create(props: Self::Properties, _: ComponentLink<Self>) -> Self {
        Self { math_rendered: Default::default(), props }
    }

    fn update(&mut self, _: Self::Message) -> ShouldRender {
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        match self.props == props {
            true => false,
            false => {
                self.props = props;
                true
            }
        }
    }

    fn view(&self) -> Html {
        let renderer = self.create_renderer();
        let t = yew::utils::document().create_element("div").unwrap();
        t.set_inner_html(&renderer.render(&self.props.math));
        Html::VRef(t.first_child().unwrap().into())
    }
    #[cfg(feature = "auto-cdn")]
    fn rendered(&mut self, first_render: bool) {
        if first_render {
            self.load_cdn().unwrap_or_default()
        }
    }
}

impl KaTeX {
    pub fn create_renderer(&self) -> KaTeXOptions {
        let mut render = if self.props.inline {
            KaTeXOptions::inline_mode()
        } else {
            KaTeXOptions::display_mode()
        };
        render.set_output_format(&self.props.output);
        return render;
    }
    pub fn load_cdn(&self) -> Result<(), std::io::Error> {
        // <link rel="stylesheet" href="https://unpkg.com/katex@0.12.0/dist/katex.min.css">
        if let None = document().get_element_by_id("cdn-katex") {
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
