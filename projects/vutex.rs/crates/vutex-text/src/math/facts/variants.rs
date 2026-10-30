//! OpenType `MathVariants`: prepared size variants and glyph assemblies.
//!
//! A math font never scales a delimiter or a large operator. It publishes
//! larger prepared designs ([`MathVariant`]) and, for sizes beyond the last
//! prepared design, a recipe that repeats one part ([`GlyphAssembly`]).
//!
//! These types are the only place that answers "which design reaches this
//! target size". Every number in them is read from the font's `MATH` table or
//! from `hmtx`/bounds; a fact the font does not publish stays `None` instead of
//! being estimated.

use std::collections::BTreeMap;

use vutex_types::math::Fixed;

use super::GlyphId;

/// Axis along which a construction grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StretchAxis {
    /// The construction grows upward, which is how delimiters and `\sum` grow.
    Vertical,
    /// The construction grows rightward, which is how wide arrows and
    /// over/under-braces grow.
    Horizontal,
}

/// One prepared larger design of a math glyph.
///
/// These are the `MathGlyphVariantRecord`s of the font plus the facts the font
/// publishes about the design itself.
#[derive(Debug, Clone, PartialEq)]
pub struct MathVariant {
    /// The prepared design's glyph.
    pub glyph: GlyphId,
    /// Horizontal pen advance from `hmtx`, in em.
    ///
    /// `None` when the font publishes no width for the glyph. A vertical
    /// variant's pen advance is unrelated to how tall it is: the stacking size
    /// is [`MathVariant::stretch_measurement`].
    pub advance: Option<Fixed>,
    /// The font's `AdvanceMeasurement`, in em.
    ///
    /// This is the size the variant reaches along the stretch axis, so it is
    /// the value a size search compares against the target.
    pub stretch_measurement: Fixed,
    /// Ink height above the baseline, in em, for vertical variants.
    ///
    /// `None` for horizontal variants and whenever the font publishes no
    /// bounding box for the glyph.
    pub height: Option<Fixed>,
    /// Italic correction the font publishes for this glyph, in em.
    pub italic_correction: Option<Fixed>,
}

/// One piece of a [`GlyphAssembly`] recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyPart {
    /// The part's glyph.
    pub glyph: GlyphId,
    /// Connector length at the start of the part, in em.
    pub start_connector: Fixed,
    /// Connector length at the end of the part, in em.
    pub end_connector: Fixed,
    /// Full extent of the part along the stretch axis, in em.
    pub full_advance: Fixed,
    /// Whether the part may be repeated to fill extra length.
    pub is_extender: bool,
}

/// A `GlyphAssembly`: the recipe for sizes beyond the last prepared variant.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphAssembly {
    /// Italic correction of the assembled glyph, in em, as the font publishes
    /// it for the assembly as a whole.
    pub italic_correction: Fixed,
    /// Ordered parts, smallest repeatable piece first.
    pub parts: Vec<AssemblyPart>,
}

/// Everything the font publishes about growing one glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphConstruction {
    /// Prepared variants in the order the font lists them (smallest first).
    pub variants: Vec<MathVariant>,
    /// Recipe for sizes beyond the last variant, when the font publishes one.
    pub assembly: Option<GlyphAssembly>,
}

/// The design chosen for one target size.
#[derive(Debug, Clone, PartialEq)]
pub enum StretchPlan<'a> {
    /// The smallest prepared variant that already reaches the target.
    Variant(&'a MathVariant),
    /// Parts to draw in order, when no prepared variant is large enough.
    Assembly(AssemblyPlan<'a>),
}

/// An expanded assembly: exactly which parts to draw, in drawing order.
#[derive(Debug, Clone, PartialEq)]
pub struct AssemblyPlan<'a> {
    /// Parts to draw top-to-bottom (or left-to-right) with extenders already
    /// repeated as often as the target needs.
    pub instances: Vec<&'a AssemblyPart>,
    /// Overlap assumed at every junction: the font's `MinConnectorOverlap`.
    pub overlap: Fixed,
    /// Extent the instances span along the stretch axis, in em.
    ///
    /// Computed with `overlap` at every junction, which is the largest extent
    /// the recipe can span. Layout that spreads the connectors further apart
    /// draws a shorter assembly than this number.
    pub extent: Fixed,
}

impl GlyphAssembly {
    /// Expand the recipe to the smallest assembly that reaches `target`.
    ///
    /// Returns `None` when the recipe cannot reach the target: no part is
    /// marked as an extender or repeating the extenders adds no length.
    ///
    /// Several extenders are repeated the same number of times. A font uses
    /// that shape for symmetric recipes — Latin Modern Math's brace is a hook,
    /// an extender, a middle piece, another extender and a second hook — so one
    /// extra round grows the recipe on both sides of the middle piece.
    pub fn plan(&self, target: Fixed, min_connector_overlap: Fixed) -> Option<AssemblyPlan<'_>> {
        if self.parts.is_empty() {
            return None;
        }

        let minimal: Vec<&AssemblyPart> = self.parts.iter().collect();
        let minimal_extent = assembled_extent(&minimal, min_connector_overlap);
        if minimal_extent >= target {
            return Some(AssemblyPlan { instances: minimal, overlap: min_connector_overlap, extent: minimal_extent });
        }

        let extenders: Vec<&AssemblyPart> = self.parts.iter().filter(|part| part.is_extender).collect();
        if extenders.is_empty() {
            return None;
        }
        // One extra round adds one copy of every extender and one junction per
        // copy.
        let mut round = Fixed::ZERO;
        for extender in &extenders {
            round = round + (extender.full_advance - min_connector_overlap);
        }
        if round <= Fixed::ZERO {
            return None;
        }

        let extra_rounds = ceil_ratio(target - minimal_extent, round);
        let mut instances = Vec::with_capacity(self.parts.len() + extra_rounds as usize * extenders.len());
        for part in &self.parts {
            let copies = if part.is_extender { 1 + extra_rounds } else { 1 };
            for _ in 0..copies {
                instances.push(part);
            }
        }
        let extent = assembled_extent(&instances, min_connector_overlap);
        Some(AssemblyPlan { instances, overlap: min_connector_overlap, extent })
    }
}

impl GlyphConstruction {
    /// The smallest prepared variant whose stretch measurement reaches `target`.
    ///
    /// Variants are searched rather than assumed sorted, so a font that lists
    /// them out of order still yields the smallest sufficient design.
    pub fn smallest_variant_covering(&self, target: Fixed) -> Option<&MathVariant> {
        self.variants
            .iter()
            .filter(|variant| variant.stretch_measurement >= target)
            .min_by_key(|variant| variant.stretch_measurement)
    }

    /// Choose the design for `target`: the TeX/OpenType rule of taking the
    /// smallest sufficient prepared variant, falling back to an assembly.
    pub fn stretch(&self, target: Fixed, min_connector_overlap: Fixed) -> Option<StretchPlan<'_>> {
        if let Some(variant) = self.smallest_variant_covering(target) {
            return Some(StretchPlan::Variant(variant));
        }
        let assembly = self.assembly.as_ref()?;
        assembly.plan(target, min_connector_overlap).map(StretchPlan::Assembly)
    }
}

/// All constructions the font publishes, split by stretch axis.
#[derive(Debug, Clone, PartialEq)]
pub struct MathVariants {
    /// The font's `MinConnectorOverlap`, in em.
    pub min_connector_overlap: Fixed,
    vertical: BTreeMap<GlyphId, GlyphConstruction>,
    horizontal: BTreeMap<GlyphId, GlyphConstruction>,
}

impl MathVariants {
    pub(crate) fn new(
        min_connector_overlap: Fixed,
        vertical: BTreeMap<GlyphId, GlyphConstruction>,
        horizontal: BTreeMap<GlyphId, GlyphConstruction>,
    ) -> Self {
        Self { min_connector_overlap, vertical, horizontal }
    }

    /// Vertical constructions the font publishes, in glyph order.
    pub fn vertical_constructions(&self) -> impl Iterator<Item = (GlyphId, &GlyphConstruction)> {
        self.vertical.iter().map(|(glyph, construction)| (*glyph, construction))
    }

    /// Horizontal constructions the font publishes, in glyph order.
    pub fn horizontal_constructions(&self) -> impl Iterator<Item = (GlyphId, &GlyphConstruction)> {
        self.horizontal.iter().map(|(glyph, construction)| (*glyph, construction))
    }

    /// The construction that grows `glyph` along `axis`, if the font has one.
    pub fn construction(&self, glyph: GlyphId, axis: StretchAxis) -> Option<&GlyphConstruction> {
        match axis {
            StretchAxis::Vertical => self.vertical.get(&glyph),
            StretchAxis::Horizontal => self.horizontal.get(&glyph),
        }
    }

    /// Choose the design that grows `glyph` to `target` along `axis`.
    pub fn stretch(&self, glyph: GlyphId, axis: StretchAxis, target: Fixed) -> Option<StretchPlan<'_>> {
        self.construction(glyph, axis)?.stretch(target, self.min_connector_overlap)
    }
}

/// Extent a sequence of parts spans, with one `overlap` at every junction.
fn assembled_extent(parts: &[&AssemblyPart], overlap: Fixed) -> Fixed {
    let mut total = Fixed::ZERO;
    for part in parts {
        total = total + part.full_advance;
    }
    let junctions = parts.len().saturating_sub(1) as i64;
    total - Fixed((overlap.0 as i64 * junctions) as i32)
}

/// Smallest whole number of copies that covers `value` with steps of `step`.
///
/// `value` and `step` are both positive in every call site, so plain integer
/// division rounds the way the repeat count needs.
fn ceil_ratio(value: Fixed, step: Fixed) -> i64 {
    let value = value.0 as i64;
    let step = step.0 as i64;
    (value + step - 1) / step
}

#[cfg(test)]
mod tests {
    use super::*;

    fn em(value: f64) -> Fixed {
        Fixed::from_f64(value)
    }

    fn part(glyph: u16, full_advance: f64, is_extender: bool) -> AssemblyPart {
        AssemblyPart {
            glyph: GlyphId::new(glyph),
            start_connector: em(0.1),
            end_connector: em(0.1),
            full_advance: em(full_advance),
            is_extender,
        }
    }

    /// Synthetic recipe: one top piece, one repeater, one bottom piece.
    fn brace_assembly() -> GlyphAssembly {
        GlyphAssembly {
            italic_correction: em(0.02),
            parts: vec![part(10, 0.5, false), part(11, 0.25, true), part(12, 0.5, false)],
        }
    }

    #[test]
    fn assembly_repeats_the_extender_until_the_target_is_reached() {
        let assembly = brace_assembly();
        let overlap = em(0.05);
        // One copy of every part spans 1.25 - 2 * 0.05 = 1.15 em.
        let plan = assembly.plan(em(2.0), overlap).expect("recipe must reach 2 em");
        let extenders = plan.instances.iter().filter(|part| part.is_extender).count();
        assert_eq!(extenders, 1 + ceil_ratio(em(2.0) - em(1.15), em(0.25) - overlap) as usize);
        assert_eq!(plan.instances.len(), 2 + extenders);
        assert!(plan.extent >= em(2.0), "assembled extent {} must reach the target", plan.extent.to_f64());
        assert!(plan.extent - em(2.0) < em(0.25), "assembly must not overshoot more than one extender");
    }

    #[test]
    fn assembly_keeps_the_minimal_recipe_when_it_already_fits() {
        let assembly = brace_assembly();
        let plan = assembly.plan(em(0.9), em(0.05)).expect("minimal recipe fits");
        assert_eq!(plan.instances.len(), 3);
        assert_eq!(plan.instances.iter().filter(|part| part.is_extender).count(), 1);
        assert!(plan.extent >= em(0.9));
    }

    #[test]
    fn assembly_without_a_repeater_cannot_grow() {
        let assembly =
            GlyphAssembly { italic_correction: Fixed::ZERO, parts: vec![part(10, 0.4, false), part(11, 0.4, false)] };
        assert!(assembly.plan(em(0.5), Fixed::ZERO).is_some(), "fixed recipe covers 0.8 em");
        assert!(assembly.plan(em(2.0), Fixed::ZERO).is_none(), "fixed recipe cannot reach 2 em");
    }

    #[test]
    fn several_extenders_grow_together_so_a_symmetric_recipe_stays_symmetric() {
        // Shape of Latin Modern Math's brace: hook, extender, middle, extender, hook.
        let assembly = GlyphAssembly {
            italic_correction: Fixed::ZERO,
            parts: vec![
                part(10, 0.5, false),
                part(11, 0.25, true),
                part(12, 0.5, false),
                part(13, 0.25, true),
                part(14, 0.5, false),
            ],
        };
        let overlap = em(0.05);
        let plan = assembly.plan(em(3.0), overlap).expect("recipe must reach 3 em");
        let upper = plan.instances.iter().filter(|part| part.glyph == GlyphId::new(11)).count();
        let lower = plan.instances.iter().filter(|part| part.glyph == GlyphId::new(13)).count();
        assert_eq!(upper, lower, "both extenders repeat the same number of times");
        assert!(upper >= 2, "the recipe must actually grow, got {upper} copies");
        assert!(plan.extent >= em(3.0), "assembled extent {} must reach the target", plan.extent.to_f64());
    }

    #[test]
    fn assembly_that_gains_nothing_from_repeating_is_rejected() {
        let assembly = GlyphAssembly { italic_correction: Fixed::ZERO, parts: vec![part(10, 0.6, false), part(11, 0.1, true)] };
        // The extender is no longer than the overlap it consumes.
        assert!(assembly.plan(em(1.0), em(0.1)).is_none());
    }

    #[test]
    fn smallest_sufficient_variant_wins_even_when_the_font_lists_them_unsorted() {
        let construction = GlyphConstruction {
            variants: vec![
                MathVariant {
                    glyph: GlyphId::new(1),
                    advance: Some(em(0.3)),
                    stretch_measurement: em(1.5),
                    height: Some(em(1.4)),
                    italic_correction: None,
                },
                MathVariant {
                    glyph: GlyphId::new(2),
                    advance: Some(em(0.4)),
                    stretch_measurement: em(1.0),
                    height: Some(em(0.9)),
                    italic_correction: None,
                },
            ],
            assembly: None,
        };
        let plan = construction.stretch(em(0.8), Fixed::ZERO).expect("a variant reaches 0.8 em");
        match plan {
            StretchPlan::Variant(variant) => {
                assert_eq!(variant.glyph, GlyphId::new(2), "1.0 em is the smallest sufficient variant")
            }
            other => panic!("expected a variant, got {other:?}"),
        }
        assert!(construction.stretch(em(2.0), Fixed::ZERO).is_none(), "no design reaches 2 em");
    }

    #[test]
    fn variant_is_preferred_over_an_assembly_that_would_also_fit() {
        let construction = GlyphConstruction {
            variants: vec![MathVariant {
                glyph: GlyphId::new(7),
                advance: Some(em(0.5)),
                stretch_measurement: em(1.2),
                height: Some(em(1.1)),
                italic_correction: Some(em(0.03)),
            }],
            assembly: Some(brace_assembly()),
        };
        match construction.stretch(em(1.0), em(0.05)) {
            Some(StretchPlan::Variant(variant)) => assert_eq!(variant.glyph, GlyphId::new(7)),
            other => panic!("expected the prepared variant first, got {other:?}"),
        }
        match construction.stretch(em(3.0), em(0.05)) {
            Some(StretchPlan::Assembly(plan)) => assert!(plan.extent >= em(3.0)),
            other => panic!("expected an assembly above the tallest variant, got {other:?}"),
        }
    }
}
