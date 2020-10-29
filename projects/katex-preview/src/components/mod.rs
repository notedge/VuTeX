use dioxus::{events::FormEvent, prelude::*};

use dioxus_katex::{use_katex, use_katex_display, KaTeXOptions, UseKatex};

pub fn Editor(cx: Scope) -> Element {
    let place_holder = r#"K_{0}-K_{1}=\frac{E}{c^2}\frac{v^2}{2}"#;
    let (text, text_set) = use_state(&cx, || place_holder.to_string());
    let katex = use_katex_display(&cx);
    let is_display = DisplayToggle(katex);
    let math = katex.compile(text);
    cx.render(rsx!(
        div {
            class: "flex flex-column",
            div {
                class: "form-control flex-1",
                textarea {
                    class: "textarea h-screen textarea-bordered textarea-primary",
                    id: "editor",
                    placeholder: "{place_holder}",
                    oninput: move |e| text_set(e.value.to_owned()),
                    value: "{text}",
                }
            }
            div {
                class: "flex-1 ml-2 mr-2",
                math
            }
        }
        div {
            class: "form-control",
            is_display
            a {
                href: "https://github.com/oovm/tailwind-rs/issues",
                target: "_blank",
                button {
                    class: "py-2 px-4 mr-2 mb-2 text-sm font-medium text-gray-900 bg-white rounded-lg border border-gray-200 hover:bg-gray-100 hover:text-blue-700 focus:z-10 focus:ring-2 focus:ring-blue-700 focus:text-blue-700 dark:bg-gray-800 dark:text-gray-400 dark:border-gray-600 dark:hover:text-white dark:hover:bg-gray-700",
                    r#type: "button",
                    "Report bug on github"
                }
            }
        }
    ))
}

fn DisplayToggle(ctx: &UseKatex) -> LazyNodes {
    let v = ctx.get_config().display_mode;
    let click = move |e: FormEvent| match e.value.as_str() {
        "true" => ctx.set_display_mode(),
        "false" => ctx.set_inline_mode(),
        _ => {},
    };
    rsx!(
        label {
            class: "cursor-pointer label",
            span {
                class: "label-text",
                "Display Mode"
            }
            input {
                r#type: "checkbox",
                class: "toggle",
                checked: "{v}",
                oninput: click
            }
        }
    )
}
// fn ModeSelect(vtx: &UseKatex) -> LazyNodes {
//     let v = vtx.get_mode();
//     rsx!(
//         label {
//             class: "cursor-pointer label",
//             span {
//                 class: "label-text",
//                 "Compile Mode"
//             }
//             select {
//                 class: "select select-primary w-full max-w-xs",
//                 value: "{v}",
//                 onchange: move |e| vtx.set_mode(e),
//                 option {
//                     value: "m",
//                     "Normal"
//                 }
//                 option {
//                     value: "i",
//                     "Inline"
//                 }
//                 option {
//                     value: "s",
//                     "Scoped"
//                 }
//                 option {
//                     value: "k",
//                     "DataKey"
//                 }
//                 option {
//                     value: "v",
//                     "DataValue"
//                 }
//             }
//         }
//     )
// }
