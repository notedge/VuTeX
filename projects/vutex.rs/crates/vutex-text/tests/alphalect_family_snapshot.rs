//! Per-character snapshot of the math alphalect contract under both embedded families.
//!
//! `math_alphalect` redirects an authored letter to the codepoint of its
//! dedicated face, and the font has the final say: a redirect only typesets
//! when the selected family's character map covers the target. This snapshot
//! pins, for every redirectable style and every source letter, the resolved
//! face and each family's coverage of it, so neither a block-offset regression
//! nor a font swap can silently change which glyph a math letter becomes.

use vutex_text::math::{MathFontFacts, MathFontFamily};
use vutex_types::math::{FontStyle, alphalect_sources, alphalect_styles, math_alphalect};

/// The pinned snapshot: one style per four lines — the style name, the
/// resolved face per source letter (`_` where the style keeps the letter
/// upright), then the `XITS` and `LMM` coverage of those faces (`#` covered,
/// `-` the family lacks the face so the caller falls back, `.` no face to
/// cover).
const SNAPSHOT: &str = "Math
\u{1D44E}\u{1D44F}\u{1D450}\u{1D451}\u{1D452}\u{1D453}\u{1D454}\u{210E}\u{1D456}\u{1D457}\u{1D458}\u{1D459}\u{1D45A}\u{1D45B}\u{1D45C}\u{1D45D}\u{1D45E}\u{1D45F}\u{1D460}\u{1D461}\u{1D462}\u{1D463}\u{1D464}\u{1D465}\u{1D466}\u{1D467}\u{1D434}\u{1D435}\u{1D436}\u{1D437}\u{1D438}\u{1D439}\u{1D43A}\u{1D43B}\u{1D43C}\u{1D43D}\u{1D43E}\u{1D43F}\u{1D440}\u{1D441}\u{1D442}\u{1D443}\u{1D444}\u{1D445}\u{1D446}\u{1D447}\u{1D448}\u{1D449}\u{1D44A}\u{1D44B}\u{1D44C}\u{1D44D}\u{1D6FC}\u{1D6FD}\u{1D6FE}\u{1D6FF}\u{1D700}\u{1D701}\u{1D702}\u{1D703}\u{1D704}\u{1D705}\u{1D706}\u{1D707}\u{1D708}\u{1D709}\u{1D70A}\u{1D70B}\u{1D70C}\u{1D71B}\u{1D70D}\u{1D70E}\u{1D70F}\u{1D710}\u{1D711}\u{1D712}\u{1D713}\u{1D716}\u{1D717}\u{1D71C}\u{1D71A}
#################################################################################
#################################################################################
Italic
\u{1D44E}\u{1D44F}\u{1D450}\u{1D451}\u{1D452}\u{1D453}\u{1D454}\u{210E}\u{1D456}\u{1D457}\u{1D458}\u{1D459}\u{1D45A}\u{1D45B}\u{1D45C}\u{1D45D}\u{1D45E}\u{1D45F}\u{1D460}\u{1D461}\u{1D462}\u{1D463}\u{1D464}\u{1D465}\u{1D466}\u{1D467}\u{1D434}\u{1D435}\u{1D436}\u{1D437}\u{1D438}\u{1D439}\u{1D43A}\u{1D43B}\u{1D43C}\u{1D43D}\u{1D43E}\u{1D43F}\u{1D440}\u{1D441}\u{1D442}\u{1D443}\u{1D444}\u{1D445}\u{1D446}\u{1D447}\u{1D448}\u{1D449}\u{1D44A}\u{1D44B}\u{1D44C}\u{1D44D}\u{1D6FC}\u{1D6FD}\u{1D6FE}\u{1D6FF}\u{1D700}\u{1D701}\u{1D702}\u{1D703}\u{1D704}\u{1D705}\u{1D706}\u{1D707}\u{1D708}\u{1D709}\u{1D70A}\u{1D70B}\u{1D70C}\u{1D71B}\u{1D70D}\u{1D70E}\u{1D70F}\u{1D710}\u{1D711}\u{1D712}\u{1D713}\u{1D716}\u{1D717}\u{1D71C}\u{1D71A}
#################################################################################
#################################################################################
Bold
\u{1D41A}\u{1D41B}\u{1D41C}\u{1D41D}\u{1D41E}\u{1D41F}\u{1D420}\u{1D421}\u{1D422}\u{1D423}\u{1D424}\u{1D425}\u{1D426}\u{1D427}\u{1D428}\u{1D429}\u{1D42A}\u{1D42B}\u{1D42C}\u{1D42D}\u{1D42E}\u{1D42F}\u{1D430}\u{1D431}\u{1D432}\u{1D433}\u{1D400}\u{1D401}\u{1D402}\u{1D403}\u{1D404}\u{1D405}\u{1D406}\u{1D407}\u{1D408}\u{1D409}\u{1D40A}\u{1D40B}\u{1D40C}\u{1D40D}\u{1D40E}\u{1D40F}\u{1D410}\u{1D411}\u{1D412}\u{1D413}\u{1D414}\u{1D415}\u{1D416}\u{1D417}\u{1D418}\u{1D419}_____________________________
####################################################.............................
####################################################.............................
SansSerif
\u{1D5BA}\u{1D5BB}\u{1D5BC}\u{1D5BD}\u{1D5BE}\u{1D5BF}\u{1D5C0}\u{1D5C1}\u{1D5C2}\u{1D5C3}\u{1D5C4}\u{1D5C5}\u{1D5C6}\u{1D5C7}\u{1D5C8}\u{1D5C9}\u{1D5CA}\u{1D5CB}\u{1D5CC}\u{1D5CD}\u{1D5CE}\u{1D5CF}\u{1D5D0}\u{1D5D1}\u{1D5D2}\u{1D5D3}\u{1D5A0}\u{1D5A1}\u{1D5A2}\u{1D5A3}\u{1D5A4}\u{1D5A5}\u{1D5A6}\u{1D5A7}\u{1D5A8}\u{1D5A9}\u{1D5AA}\u{1D5AB}\u{1D5AC}\u{1D5AD}\u{1D5AE}\u{1D5AF}\u{1D5B0}\u{1D5B1}\u{1D5B2}\u{1D5B3}\u{1D5B4}\u{1D5B5}\u{1D5B6}\u{1D5B7}\u{1D5B8}\u{1D5B9}_____________________________
####################################################.............................
####################################################.............................
Monospace
\u{1D68A}\u{1D68B}\u{1D68C}\u{1D68D}\u{1D68E}\u{1D68F}\u{1D690}\u{1D691}\u{1D692}\u{1D693}\u{1D694}\u{1D695}\u{1D696}\u{1D697}\u{1D698}\u{1D699}\u{1D69A}\u{1D69B}\u{1D69C}\u{1D69D}\u{1D69E}\u{1D69F}\u{1D6A0}\u{1D6A1}\u{1D6A2}\u{1D6A3}\u{1D670}\u{1D671}\u{1D672}\u{1D673}\u{1D674}\u{1D675}\u{1D676}\u{1D677}\u{1D678}\u{1D679}\u{1D67A}\u{1D67B}\u{1D67C}\u{1D67D}\u{1D67E}\u{1D67F}\u{1D680}\u{1D681}\u{1D682}\u{1D683}\u{1D684}\u{1D685}\u{1D686}\u{1D687}\u{1D688}\u{1D689}_____________________________
####################################################.............................
####################################################.............................
Calligraphic
__________________________\u{1D49C}\u{212C}\u{1D49E}\u{1D49F}\u{2130}\u{2131}\u{1D4A2}\u{210B}\u{2110}\u{1D4A5}\u{1D4A6}\u{2112}\u{2133}\u{1D4A9}\u{1D4AA}\u{1D4AB}\u{1D4AC}\u{211B}\u{1D4AE}\u{1D4AF}\u{1D4B0}\u{1D4B1}\u{1D4B2}\u{1D4B3}\u{1D4B4}\u{1D4B5}_____________________________
..........................##########################.............................
..........................##########################.............................
DoubleStruck
\u{1D552}\u{1D553}\u{1D554}\u{1D555}\u{1D556}\u{1D557}\u{1D558}\u{1D559}\u{1D55A}\u{1D55B}\u{1D55C}\u{1D55D}\u{1D55E}\u{1D55F}\u{1D560}\u{1D561}\u{1D562}\u{1D563}\u{1D564}\u{1D565}\u{1D566}\u{1D567}\u{1D568}\u{1D569}\u{1D56A}\u{1D56B}\u{1D538}\u{1D539}\u{2102}\u{1D53B}\u{1D53C}\u{1D53D}\u{1D53E}\u{210D}\u{1D540}\u{1D541}\u{1D542}\u{1D543}\u{1D544}\u{2115}\u{1D546}\u{2119}\u{211A}\u{211D}\u{1D54A}\u{1D54B}\u{1D54C}\u{1D54D}\u{1D54E}\u{1D54F}\u{1D550}\u{2124}_____________________________
####################################################.............................
####################################################.............................
Fraktur
\u{1D51E}\u{1D51F}\u{1D520}\u{1D521}\u{1D522}\u{1D523}\u{1D524}\u{1D525}\u{1D526}\u{1D527}\u{1D528}\u{1D529}\u{1D52A}\u{1D52B}\u{1D52C}\u{1D52D}\u{1D52E}\u{1D52F}\u{1D530}\u{1D531}\u{1D532}\u{1D533}\u{1D534}\u{1D535}\u{1D536}\u{1D537}\u{1D504}\u{1D505}\u{212D}\u{1D507}\u{1D508}\u{1D509}\u{1D50A}\u{210C}\u{2111}\u{1D50D}\u{1D50E}\u{1D50F}\u{1D510}\u{1D511}\u{1D512}\u{1D513}\u{1D514}\u{211C}\u{1D516}\u{1D517}\u{1D518}\u{1D519}\u{1D51A}\u{1D51B}\u{1D51C}\u{2128}_____________________________
####################################################.............................
####################################################.............................
";

/// Build one style's three snapshot lines: faces, then each family's coverage.
fn style_snapshot(style: &FontStyle, xits: &MathFontFacts, lm: &MathFontFacts) -> String {
    let mut faces = String::new();
    let mut xits_cov = String::new();
    let mut lm_cov = String::new();
    for c in alphalect_sources() {
        match math_alphalect(*style, *c) {
            Some(face) => {
                faces.push(face);
                xits_cov.push(if xits.glyph_for_char(face).is_some() { '#' } else { '-' });
                lm_cov.push(if lm.glyph_for_char(face).is_some() { '#' } else { '-' });
            }
            None => {
                faces.push('_');
                xits_cov.push('.');
                lm_cov.push('.');
            }
        }
    }
    format!("{style:?}\n{faces}\n{xits_cov}\n{lm_cov}\n")
}

fn build_snapshot() -> String {
    let xits = MathFontFacts::for_family(MathFontFamily::XitsMath).expect("the XITS font carries a MATH table");
    let lm = MathFontFacts::for_family(MathFontFamily::LmMath).expect("the LM Math font carries a MATH table");
    alphalect_styles().iter().map(|style| style_snapshot(style, &xits, &lm)).collect()
}

#[test]
fn alphalect_faces_and_family_coverage_match_the_snapshot() {
    let actual = build_snapshot();
    assert_eq!(actual, SNAPSHOT, "the alphabet mapping must keep one per-character snapshot under both families");
}

/// Refresh helper: prints the snapshot with non-ASCII faces escaped so the
/// pinned constant stays ASCII. Run with `--nocapture` after changing the
/// alphalect contract or the embedded fonts, then paste over `SNAPSHOT`.
#[test]
fn emit_escaped_snapshot() {
    let mut out = String::new();
    for c in build_snapshot().chars() {
        match c {
            '\n' | '#' | '-' | '.' | '_' => out.push(c),
            face => out.push_str(&format!("\\u{{{:X}}}", face as u32)),
        }
    }
    println!("{out}");
}
