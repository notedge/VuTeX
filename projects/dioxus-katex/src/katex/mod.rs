use std::{
    cell::{Ref, RefCell, RefMut},
    fmt::{Debug, Formatter},
    rc::Rc,
};

use dioxus::prelude::*;
use dioxus_elements::{div, GlobalAttributes};

use katex_wasmbind::KaTeXOptions;

pub mod builder;

/// A hook which keeping the context of KaTeX formula.
pub struct UseKatex {
    katex: Rc<RefCell<KaTeXOptions>>,
    updater: Rc<dyn Fn() + 'static>,
}

impl UseKatex {
    /// Get all config of KaTeX formula.
    pub fn get_config(&self) -> Ref<'_, KaTeXOptions> {
        self.katex.borrow()
    }
    /// Get the mutable reference of all config.
    pub fn get_config_mut(&self) -> RefMut<'_, KaTeXOptions> {
        self.katex.borrow_mut()
    }
    /// Set the formula to inline mode.
    pub fn set_inline_mode(&self) {
        self.get_config_mut().display_mode = false;
        self.needs_update();
    }
    /// Set the formula to display mode.
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
    /// Compile the formula to HTML.
    ///
    /// Never fails even if the formula is invalid.
    pub fn compile(&self, input: &str) -> LazyNodes {
        let config = self.katex.borrow_mut();
        let out = config.render(input);
        LazyNodes::new_some(move |cx: NodeFactory| -> VNode {
            cx.element(
                div,
                cx.bump().alloc([]),
                cx.bump().alloc([div.dangerous_inner_html(cx, format_args!("{out}", out = out))]),
                cx.bump().alloc([]),
                None,
            )
        })
    }
}
