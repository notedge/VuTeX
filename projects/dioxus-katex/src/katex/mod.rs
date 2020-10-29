use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use dioxus::prelude::*;

use katex_wasmbind::KaTeXOptions;

pub mod builder;

pub struct UseKatex {
    katex: Rc<RefCell<KaTeXOptions>>,
    updater: Rc<dyn Fn() + 'static>,
}

impl UseKatex {
    pub fn get_config(&self) -> Ref<'_, KaTeXOptions> {
        self.katex.borrow()
    }
    pub fn get_config_mut(&self) -> RefMut<'_, KaTeXOptions> {
        self.katex.borrow_mut()
    }
    pub fn set_inline_mode(&self) {
        self.get_config_mut().display_mode = false;
        self.needs_update();
    }
    pub fn set_display_mode(&self) {
        self.get_config_mut().display_mode = true;
        self.needs_update();
    }
    /// Notify the scheduler to re-render the component.
    pub fn needs_update(&self) {
        (self.updater)();
    }
}

impl UseKatex {
    pub fn compile(&self, input: &str) -> LazyNodes {
        let config = self.katex.borrow_mut();
        let out = config.render(input);
        rsx! {
            div {
                dangerous_inner_html: "{out}"
            }
        }
    }
}
