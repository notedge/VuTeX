use std::sync::Arc;
use dashmap::DashMap;
use async_trait::async_trait;
pub use rutex_types::{
    RuTeXError, Result, GlyphKey, Fixed, MathConstant, GlyphMetrics, FontMetricsData,
    DelimiterConstruction, DelimiterVariant, DelimiterComponent
};

#[async_trait]
pub trait FontLoader: Send + Sync {
    async fn load_font_data(&self, family: &str) -> Result<Arc<Vec<u8>>>;
}

pub struct FileFontLoader {
    base_path: std::path::PathBuf,
}

impl FileFontLoader {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Self {
        Self { base_path: base_path.into() }
    }
}

#[async_trait]
impl FontLoader for FileFontLoader {
    async fn load_font_data(&self, family: &str) -> Result<Arc<Vec<u8>>> {
        let path = self.base_path.join(format!("{}.ttf", family));
        let data = std::fs::read(&path).map_err(RuTeXError::io_error)?;
        Ok(Arc::new(data))
    }
}

pub struct FontMetricsSystem {
    loader: Option<Arc<dyn FontLoader>>,
    font_data_cache: DashMap<String, Arc<Vec<u8>>>,
    metrics_cache: DashMap<String, FontMetricsData>,
}

impl FontMetricsSystem {
    pub fn new(loader: Arc<dyn FontLoader>) -> Self {
        Self {
            loader: Some(loader),
            font_data_cache: DashMap::new(),
            metrics_cache: DashMap::new(),
        }
    }

    /// Create a system with only pre-computed metrics, no font loading support.
    pub fn new_with_metrics(data: FontMetricsData) -> Self {
        let system = Self {
            loader: None,
            font_data_cache: DashMap::new(),
            metrics_cache: DashMap::new(),
        };
        system.add_metrics_data(data);
        system
    }

    /// Hydrate the cache with pre-calculated metrics (AOT).
    pub fn add_metrics_data(&self, data: FontMetricsData) {
        self.metrics_cache.insert(data.family.clone(), data);
    }

    /// Insert a single glyph's metrics into the cache. 
    /// If the family doesn't exist in the cache, it creates a new FontMetricsData entry.
    pub fn insert_metrics(&self, key: GlyphKey, metrics: GlyphMetrics) {
        let family = key.font_family.clone().unwrap_or_else(|| "default".to_string());
        let mut entry = self.metrics_cache.entry(family.clone()).or_insert_with(|| {
            FontMetricsData::new(family, 1000) // Default 1000 units per em
        });
        entry.glyphs.insert(key, metrics);
    }

    pub async fn get_metrics(&self, key: &GlyphKey) -> Result<GlyphMetrics> {
        let family = key.font_family.as_deref().unwrap_or("default");
        
        // 1. Check if we have this specific glyph in the metrics cache
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(metrics) = data.glyphs.get(key) {
                return Ok(*metrics);
            }
        }

        // 2. If not in cache and loader available, load and parse
        if self.loader.is_none() {
            return Err(RuTeXError::font_error(
                family,
                format!("Metrics for '{:?}' not found in cache and no loader available", key.char)
            ));
        }

        let data = self.get_font_data(family).await?;
        let metrics = self.parse_metrics(&data, key)?;
        
        // 3. Cache it back into FontMetricsData
        let mut entry = self.metrics_cache.entry(family.to_string()).or_insert_with(|| {
            let face = ttf_parser::Face::parse(&data, 0).unwrap();
            FontMetricsData::new(family.to_string(), face.units_per_em())
        });
        entry.glyphs.insert(key.clone(), metrics);
        
        Ok(metrics)
    }

    pub async fn get_delimiter_construction(&self, key: &GlyphKey) -> Result<DelimiterConstruction> {
        let family = key.font_family.as_deref().unwrap_or("default");
        
        // 1. Check cache
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(construction) = data.delimiters.get(key) {
                return Ok(construction.clone());
            }
        }

        // 2. Load and parse
        let data = self.get_font_data(family).await?;
        let construction = self.parse_delimiter_construction(&data, key)?;
        
        // 3. Cache it
        if let Some(mut entry) = self.metrics_cache.get_mut(family) {
            entry.delimiters.insert(key.clone(), construction.clone());
        }
        
        Ok(construction)
    }

    fn parse_delimiter_construction(&self, data: &[u8], key: &GlyphKey) -> Result<DelimiterConstruction> {
        let face = ttf_parser::Face::parse(data, 0)
            .map_err(|e| RuTeXError::font_error(key.font_family.as_deref().unwrap_or("default"), format!("Failed to parse font: {}", e)))?;
        
        let glyph_id = if let Some(gid) = key.glyph_id {
            ttf_parser::GlyphId(gid)
        } else if let Some(c) = key.char {
            face.glyph_index(c).ok_or_else(|| RuTeXError::font_error(key.font_family.as_deref().unwrap_or("default"), format!("Glyph not found for char: {}", c)))?
        } else {
            return Err(RuTeXError::font_error(key.font_family.as_deref().unwrap_or("default"), "GlyphKey must have either char or glyph_id"));
        };
        
        let upem = face.units_per_em() as f64;
        let mut variants = Vec::new();
        let mut components = Vec::new();

        /*
        if let Some(math) = face.tables().math {
            if let Some(variants_table) = math.variants {
                // Try vertical construction first
                if let Some(construction) = variants_table.vertical_variants(glyph_id) {
                    for variant in construction.variants {
                        variants.push(DelimiterVariant {
                            glyph: GlyphKey::from_gid(variant.variant_glyph.0, key.font_family.clone(), key.style),
                        });
                    }
                    
                    if let Some(assembly) = construction.assembly {
                        for part in assembly.parts {
                            components.push(DelimiterComponent {
                                glyph: GlyphKey::from_gid(part.glyph_id.0, key.font_family.clone(), key.style),
                                is_extender: part.part_flags.extender(),
                                start_connector: Fixed::from_f64(part.start_connector_length.value as f64 / upem),
                                end_connector: Fixed::from_f64(part.end_connector_length.value as f64 / upem),
                                full_advance: Fixed::from_f64(part.full_advance.value as f64 / upem),
                            });
                        }
                    }
                }
            }
        }
        */

        Ok(DelimiterConstruction { variants, components })
    }

    pub async fn get_math_constant(&self, family: &str, constant: MathConstant) -> Result<Fixed> {
        // 1. Check cache
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(val) = data.constants.get(&constant) {
                return Ok(*val);
            }
        }

        // 2. Load and parse
        if self.loader.is_none() {
            return Err(RuTeXError::font_error(
                family,
                format!("Constant {:?} not found in cache and no loader available", constant)
            ));
        }

        let data = self.get_font_data(family).await?;
        let face = ttf_parser::Face::parse(&data, 0)
            .map_err(|e| RuTeXError::font_error(family, format!("Failed to parse font: {}", e)))?;

        let upem = face.units_per_em() as f64;
        let math = face.tables().math;
        let constants = math.and_then(|m| m.constants);

        let result = if let Some(constants) = constants {
            let value = match constant {
                MathConstant::ScriptPercentScaleDown => constants.script_percent_scale_down() as i32,
                MathConstant::ScriptScriptPercentScaleDown => constants.script_script_percent_scale_down() as i32,
                MathConstant::DelimitedSubFormulaMinHeight => constants.delimited_sub_formula_min_height() as i32,
                MathConstant::DisplayOperatorMinHeight => constants.display_operator_min_height() as i32,
                MathConstant::MathLeading => constants.math_leading().value as i32,
                MathConstant::AxisHeight => constants.axis_height().value as i32,
                MathConstant::AccentBaseHeight => constants.accent_base_height().value as i32,
                MathConstant::FlattenedAccentBaseHeight => constants.flattened_accent_base_height().value as i32,
                MathConstant::SubscriptShiftDown => constants.subscript_shift_down().value as i32,
                MathConstant::SubscriptTopMax => constants.subscript_top_max().value as i32,
                MathConstant::SubscriptBaselineDropMin => constants.subscript_baseline_drop_min().value as i32,
                MathConstant::SuperscriptShiftUp => constants.superscript_shift_up().value as i32,
                MathConstant::SuperscriptShiftUpCramped => constants.superscript_shift_up_cramped().value as i32,
                MathConstant::SuperscriptBottomMin => constants.superscript_bottom_min().value as i32,
                MathConstant::SuperscriptBaselineDropMax => constants.superscript_baseline_drop_max().value as i32,
                MathConstant::SubSuperscriptGapMin => constants.sub_superscript_gap_min().value as i32,
                MathConstant::SuperscriptBottomMaxWithSubscript => constants.superscript_bottom_max_with_subscript().value as i32,
                MathConstant::SpaceAfterScript => constants.space_after_script().value as i32,
                MathConstant::UpperLimitGapMin => constants.upper_limit_gap_min().value as i32,
                MathConstant::UpperLimitBaselineRiseMin => constants.upper_limit_baseline_rise_min().value as i32,
                MathConstant::LowerLimitGapMin => constants.lower_limit_gap_min().value as i32,
                MathConstant::LowerLimitBaselineDropMin => constants.lower_limit_baseline_drop_min().value as i32,
                MathConstant::StackTopShiftUp => constants.stack_top_shift_up().value as i32,
                MathConstant::StackTopDisplayStyleShiftUp => constants.stack_top_display_style_shift_up().value as i32,
                MathConstant::StackBottomShiftDown => constants.stack_bottom_shift_down().value as i32,
                MathConstant::StackBottomDisplayStyleShiftDown => constants.stack_bottom_display_style_shift_down().value as i32,
                MathConstant::StackGapMin => constants.stack_gap_min().value as i32,
                MathConstant::StackDisplayStyleGapMin => constants.stack_display_style_gap_min().value as i32,
                MathConstant::StretchStackTopShiftUp => constants.stretch_stack_top_shift_up().value as i32,
                MathConstant::StretchStackBottomShiftDown => constants.stretch_stack_bottom_shift_down().value as i32,
                MathConstant::StretchStackGapAboveMin => constants.stretch_stack_gap_above_min().value as i32,
                MathConstant::StretchStackGapBelowMin => constants.stretch_stack_gap_below_min().value as i32,
                MathConstant::FractionNumeratorShiftUp => constants.fraction_numerator_shift_up().value as i32,
                MathConstant::FractionNumeratorDisplayStyleShiftUp => constants.fraction_numerator_display_style_shift_up().value as i32,
                MathConstant::FractionDenominatorShiftDown => constants.fraction_denominator_shift_down().value as i32,
                MathConstant::FractionDenominatorDisplayStyleShiftDown => constants.fraction_denominator_display_style_shift_down().value as i32,
                MathConstant::FractionNumeratorGapMin => constants.fraction_numerator_gap_min().value as i32,
                MathConstant::FractionNumDisplayStyleGapMin => constants.fraction_num_display_style_gap_min().value as i32,
                MathConstant::FractionRuleThickness => constants.fraction_rule_thickness().value as i32,
                MathConstant::FractionDenominatorGapMin => constants.fraction_denominator_gap_min().value as i32,
                MathConstant::FractionDenomDisplayStyleGapMin => constants.fraction_denom_display_style_gap_min().value as i32,
                MathConstant::SkewedFractionHorizontalGap => constants.skewed_fraction_horizontal_gap().value as i32,
                MathConstant::SkewedFractionVerticalGap => constants.skewed_fraction_vertical_gap().value as i32,
                MathConstant::OverbarVerticalGap => constants.overbar_vertical_gap().value as i32,
                MathConstant::OverbarRuleThickness => constants.overbar_rule_thickness().value as i32,
                MathConstant::OverbarExtraAscender => constants.overbar_extra_ascender().value as i32,
                MathConstant::UnderbarVerticalGap => constants.underbar_vertical_gap().value as i32,
                MathConstant::UnderbarRuleThickness => constants.underbar_rule_thickness().value as i32,
                MathConstant::UnderbarExtraDescender => constants.underbar_extra_descender().value as i32,
                MathConstant::RadicalVerticalGap => constants.radical_vertical_gap().value as i32,
                MathConstant::RadicalDisplayStyleVerticalGap => constants.radical_display_style_vertical_gap().value as i32,
                MathConstant::RadicalRuleThickness => constants.radical_rule_thickness().value as i32,
                MathConstant::RadicalExtraAscender => constants.radical_extra_ascender().value as i32,
                MathConstant::RadicalKernBeforeDegree => constants.radical_kern_before_degree().value as i32,
                MathConstant::RadicalKernAfterDegree => constants.radical_kern_after_degree().value as i32,
                MathConstant::RadicalDegreeBottomRaisePercent => constants.radical_degree_bottom_raise_percent() as i32,
            };

            match constant {
                MathConstant::ScriptPercentScaleDown | 
                MathConstant::ScriptScriptPercentScaleDown | 
                MathConstant::RadicalDegreeBottomRaisePercent => {
                    Fixed::from_f64(value as f64 / 100.0)
                }
                _ => {
                    Fixed::from_f64(value as f64 / upem)
                }
            }
        } else {
            // Comprehensive default values if MATH table is missing (e.g. for KaTeX fonts)
            // Based on TeX/KaTeX standard proportions
            let default_value = match constant {
                MathConstant::ScriptPercentScaleDown => 0.7,
                MathConstant::ScriptScriptPercentScaleDown => 0.5,
                MathConstant::AxisHeight => 0.25,
                MathConstant::FractionRuleThickness => 0.06,
                MathConstant::FractionNumeratorShiftUp => 0.35,
                MathConstant::FractionNumeratorDisplayStyleShiftUp => 0.6,
                MathConstant::FractionDenominatorShiftDown => 0.35,
                MathConstant::FractionDenominatorDisplayStyleShiftDown => 0.6,
                MathConstant::FractionNumeratorGapMin => 0.05,
                MathConstant::FractionNumDisplayStyleGapMin => 0.1,
                MathConstant::FractionDenominatorGapMin => 0.05,
                MathConstant::FractionDenomDisplayStyleGapMin => 0.1,
                MathConstant::SubscriptShiftDown => 0.2,
                MathConstant::SubscriptTopMax => 0.4,
                MathConstant::SuperscriptShiftUp => 0.4,
                MathConstant::SuperscriptShiftUpCramped => 0.35,
                MathConstant::SubSuperscriptGapMin => 0.1,
                MathConstant::UpperLimitGapMin => 0.1,
                MathConstant::LowerLimitGapMin => 0.1,
                MathConstant::StackGapMin => 0.1,
                MathConstant::StackDisplayStyleGapMin => 0.3,
                MathConstant::RadicalVerticalGap => 0.06,
                MathConstant::RadicalDisplayStyleVerticalGap => 0.1,
                MathConstant::RadicalRuleThickness => 0.06,
                MathConstant::RadicalExtraAscender => 0.06,
                MathConstant::AccentBaseHeight => 0.45,
                MathConstant::OverbarRuleThickness => 0.06,
                MathConstant::OverbarVerticalGap => 0.1,
                MathConstant::UnderbarRuleThickness => 0.06,
                MathConstant::UnderbarVerticalGap => 0.1,
                _ => 0.0,
            };
            Fixed::from_f64(default_value)
        };

        // 3. Cache it
        let mut entry = self.metrics_cache.entry(family.to_string()).or_insert_with(|| {
            FontMetricsData::new(family.to_string(), face.units_per_em())
        });
        entry.constants.insert(constant, result);

        Ok(result)
    }
}

struct SvgOutlineBuilder {
    path: String,
}

impl SvgOutlineBuilder {
    fn new() -> Self {
        Self { path: String::new() }
    }
}

impl ttf_parser::OutlineBuilder for SvgOutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.path, "M {} {} ", x, -y).unwrap();
    }
    fn line_to(&mut self, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.path, "L {} {} ", x, -y).unwrap();
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.path, "Q {} {} {} {} ", x1, -y1, x, -y).unwrap();
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        use std::fmt::Write;
        write!(self.path, "C {} {} {} {} {} {} ", x1, -y1, x2, -y2, x, -y).unwrap();
    }
    fn close(&mut self) {
        self.path.push_str("Z ");
    }
}

impl FontMetricsSystem {
    /// Export all metrics for a given font and set of characters for AOT.
    pub async fn export_metrics(&self, family: &str, chars: &[char]) -> Result<FontMetricsData> {
        let data = self.get_font_data(family).await?;
        let face = ttf_parser::Face::parse(&data, 0)
            .map_err(|e| RuTeXError::font_error(family, format!("Failed to parse font: {}", e)))?;

        let upem = face.units_per_em();
        let mut metrics_data = FontMetricsData::new(family.to_string(), upem);

        // 1. Export all constants
        let all_constants = [
            MathConstant::ScriptPercentScaleDown,
            MathConstant::ScriptScriptPercentScaleDown,
            MathConstant::DelimitedSubFormulaMinHeight,
            MathConstant::DisplayOperatorMinHeight,
            MathConstant::MathLeading,
            MathConstant::AxisHeight,
            MathConstant::AccentBaseHeight,
            MathConstant::FlattenedAccentBaseHeight,
            MathConstant::SubscriptShiftDown,
            MathConstant::SubscriptTopMax,
            MathConstant::SubscriptBaselineDropMin,
            MathConstant::SuperscriptShiftUp,
            MathConstant::SuperscriptShiftUpCramped,
            MathConstant::SuperscriptBottomMin,
            MathConstant::SuperscriptBaselineDropMax,
            MathConstant::SubSuperscriptGapMin,
            MathConstant::SuperscriptBottomMaxWithSubscript,
            MathConstant::SpaceAfterScript,
            MathConstant::UpperLimitGapMin,
            MathConstant::UpperLimitBaselineRiseMin,
            MathConstant::LowerLimitGapMin,
            MathConstant::LowerLimitBaselineDropMin,
            MathConstant::StackTopShiftUp,
            MathConstant::StackTopDisplayStyleShiftUp,
            MathConstant::StackBottomShiftDown,
            MathConstant::StackBottomDisplayStyleShiftDown,
            MathConstant::StackGapMin,
            MathConstant::StackDisplayStyleGapMin,
            MathConstant::StretchStackTopShiftUp,
            MathConstant::StretchStackBottomShiftDown,
            MathConstant::StretchStackGapAboveMin,
            MathConstant::StretchStackGapBelowMin,
            MathConstant::FractionNumeratorShiftUp,
            MathConstant::FractionNumeratorDisplayStyleShiftUp,
            MathConstant::FractionDenominatorShiftDown,
            MathConstant::FractionDenominatorDisplayStyleShiftDown,
            MathConstant::FractionNumeratorGapMin,
            MathConstant::FractionNumDisplayStyleGapMin,
            MathConstant::FractionRuleThickness,
            MathConstant::FractionDenominatorGapMin,
            MathConstant::FractionDenomDisplayStyleGapMin,
            MathConstant::SkewedFractionHorizontalGap,
            MathConstant::SkewedFractionVerticalGap,
            MathConstant::OverbarVerticalGap,
            MathConstant::OverbarRuleThickness,
            MathConstant::OverbarExtraAscender,
            MathConstant::UnderbarVerticalGap,
            MathConstant::UnderbarRuleThickness,
            MathConstant::UnderbarExtraDescender,
            MathConstant::RadicalVerticalGap,
            MathConstant::RadicalDisplayStyleVerticalGap,
            MathConstant::RadicalRuleThickness,
            MathConstant::RadicalExtraAscender,
            MathConstant::RadicalKernBeforeDegree,
            MathConstant::RadicalKernAfterDegree,
            MathConstant::RadicalDegreeBottomRaisePercent,
        ];

        for c in all_constants {
            if let Ok(val) = self.get_math_constant(family, c).await {
                metrics_data.constants.insert(c, val);
            }
        }

        // 2. Export glyph metrics for requested characters
        for &ch in chars {
            let key = GlyphKey::from_char(ch, Some(family.to_string()), rutex_types::FontStyle::Normal);
            if let Ok(metrics) = self.parse_metrics(&data, &key) {
                metrics_data.glyphs.insert(key.clone(), metrics);
            }
            if let Ok(path) = self.get_glyph_path_internal(&data, &key) {
                metrics_data.glyph_paths.insert(key, path);
            }
        }

        Ok(metrics_data)
    }

    async fn get_font_data(&self, family: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(data) = self.font_data_cache.get(family) {
            return Ok(data.clone());
        }
        
        // This method is called when we NEED font data.
        // If loader is None, it's a fatal error because we expected to find data in cache.
        let loader = self.loader.as_ref().ok_or_else(|| RuTeXError::font_error(
            family,
            "No font loader available (AOT mode) and font data missing from cache"
        ))?;

        let data = loader.load_font_data(family).await?;
        self.font_data_cache.insert(family.to_string(), data.clone());
        Ok(data)
    }

    pub async fn get_glyph_path(&self, key: &GlyphKey) -> Result<String> {
        let family = key.font_family.as_deref().unwrap_or("default");
        
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(path) = data.glyph_paths.get(key) {
                return Ok(path.clone());
            }
        }

        let data = self.get_font_data(family).await?;
        let path = self.get_glyph_path_internal(&data, key)?;
        
        // Cache it
        if let Some(mut entry) = self.metrics_cache.get_mut(family) {
            entry.glyph_paths.insert(key.clone(), path.clone());
        }
        
        Ok(path)
    }

    fn get_glyph_path_internal(&self, data: &[u8], key: &GlyphKey) -> Result<String> {
        let face = ttf_parser::Face::parse(data, 0)
            .map_err(|e| RuTeXError::font_error(
                key.font_family.as_deref().unwrap_or("default"),
                format!("Failed to parse font: {}", e)
            ))?;

        let glyph_id = if let Some(gid) = key.glyph_id {
            ttf_parser::GlyphId(gid)
        } else if let Some(c) = key.char {
            face.glyph_index(c).ok_or_else(|| RuTeXError::font_error(
                key.font_family.as_deref().unwrap_or("default"),
                format!("Glyph not found for char: {}", c)
            ))?
        } else {
            return Err(RuTeXError::font_error(
                key.font_family.as_deref().unwrap_or("default"),
                "GlyphKey must have either char or glyph_id"
            ));
        };

        let mut builder = SvgOutlineBuilder::new();
        face.outline_glyph(glyph_id, &mut builder);
        
        let upem = face.units_per_em() as f32;
        // Scale down to 1em
        Ok(format!("scale({}, {}) {}", 1.0/upem, 1.0/upem, builder.path))
    }

    fn parse_metrics(&self, data: &[u8], key: &GlyphKey) -> Result<GlyphMetrics> {
        let face = ttf_parser::Face::parse(data, 0)
            .map_err(|e| RuTeXError::font_error(
                key.font_family.as_deref().unwrap_or("default"),
                format!("Failed to parse font: {}", e)
            ))?;

        let glyph_id = if let Some(gid) = key.glyph_id {
            Some(ttf_parser::GlyphId(gid))
        } else if let Some(c) = key.char {
            face.glyph_index(c).or_else(|| {
                // Fallback to '?' or space if character is missing
                face.glyph_index('?')
            }).or_else(|| {
                face.glyph_index(' ')
            })
        } else {
            None
        };

        let (glyph_id, _is_fallback) = match glyph_id {
            Some(id) => (id, false),
            None => {
                // If even fallback fails, we return zero metrics instead of error
                return Ok(GlyphMetrics {
                    width: Fixed::from_f64(0.5), // Dummy width
                    height: Fixed::from_f64(0.8),
                    depth: Fixed::from_f64(0.2),
                    italic_correction: Fixed::ZERO,
                });
            }
        };

        let upem = face.units_per_em() as f64;
        let width = Fixed::from_f64(face.glyph_hor_advance(glyph_id).unwrap_or(0) as f64 / upem);
        
        let bbox = face.glyph_bounding_box(glyph_id)
            .unwrap_or(ttf_parser::Rect { x_min: 0, y_min: 0, x_max: 0, y_max: 0 });

        let height = Fixed::from_f64(bbox.y_max as f64 / upem);
        let depth = Fixed::from_f64((-bbox.y_min).max(0) as f64 / upem);

        let mut italic_correction = Fixed::ZERO;
        
        if let Some(math) = face.tables().math {
            if let Some(glyph_info) = math.glyph_info {
                if let Some(it_corr) = glyph_info.italic_corrections {
                    if let Some(value) = it_corr.get(glyph_id) {
                        italic_correction = Fixed::from_f64(value.value as f64 / upem);
                    }
                }
            }
        }

        Ok(GlyphMetrics {
            width,
            height,
            depth,
            italic_correction,
        })
    }
}
