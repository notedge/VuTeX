use super::*;

pub struct UseKatexBuilder {}

impl Default for UseKatexBuilder {
    fn default() -> Self {
        Self {}
    }
}

impl UseKatexBuilder {
    pub fn use_tailwind<'a>(&self, cx: &'a ScopeState) -> &'a mut UseKatex {
        cx.use_hook(|_| UseKatex::new(cx, self))
    }
}

impl UseKatex {
    fn new(cx: &ScopeState, _: &UseKatexBuilder) -> Self {
        let cfg = KaTeXOptions::default();
        Self { katex: Rc::new(RefCell::new(cfg)), updater: cx.schedule_update() }
    }
}

pub fn use_katex(cx: &ScopeState) -> &UseKatex {
    UseKatexBuilder::default().use_tailwind(cx)
}
