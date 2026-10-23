pub use rutex_types::{RuTeXError, Result};
pub use rutex_layout::LayoutNode;
pub use rutex_renderer_svg::{SvgBackend, render_layout_node, LayoutBackend};

pub struct ImageBackend {
    svg_backend: SvgBackend,
    ppi: f32,
}

impl ImageBackend {
    pub fn new(width: f64, height: f64, ppi: f32) -> Self {
        Self {
            svg_backend: SvgBackend::new(width, height),
            ppi,
        }
    }

    pub fn finish(self) -> Result<Vec<u8>> {
        let svg_str = self.svg_backend.finish();
        svg_to_png(&svg_str, self.ppi)
    }
}

impl LayoutBackend for ImageBackend {
    fn render_text(&mut self, text: &str, x: f64, y: f64, font_size: f64, font_family: Option<&str>) -> Result<()> {
        self.svg_backend.render_text(text, x, y, font_size, font_family)
    }

    fn render_rect(&mut self, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
        self.svg_backend.render_rect(x, y, w, h)
    }

    fn render_path(&mut self, d: &str, x: f64, y: f64, scale: f64) -> Result<()> {
        self.svg_backend.render_path(d, x, y, scale)
    }

    fn start_group(&mut self, transform: Option<&str>) -> Result<()> {
        self.svg_backend.start_group(transform)
    }

    fn end_group(&mut self) -> Result<()> {
        self.svg_backend.end_group()
    }
}

pub fn render_to_png(node: &LayoutNode, ppi: f32) -> Result<Vec<u8>> {
    let width = node.width().to_f64();
    let height = (node.height() + node.depth()).to_f64();
    
    let mut backend = ImageBackend::new(width, height, ppi);
    render_layout_node(&mut backend, node, 0.0, node.height().to_f64())?;
    backend.finish()
}

fn svg_to_png(svg_str: &str, ppi: f32) -> Result<Vec<u8>> {
    let mut opt = resvg::usvg::Options::default();
    
    // Initialize fontdb
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    
    // Attempt to load the default font from the expected assets path
    // This is a bit of a hack but helps in the common case where the user is running from the workspace root.
    let possible_font_paths = [
        "projects/rutex/assets/fonts/default.ttf",
        "../rutex/assets/fonts/default.ttf",
        "assets/fonts/default.ttf",
    ];

    for path in possible_font_paths {
         if std::path::Path::new(path).exists() {
            let _ = fontdb.load_font_file(path);
        }
     }
     
    // Set the first loaded font as the default family for usvg
    if let Some(face) = fontdb.faces().next() {
        opt.font_family = face.families[0].0.clone();
    }
     opt.fontdb = std::sync::Arc::new(fontdb);
    
    let rtree = resvg::usvg::Tree::from_str(svg_str, &opt)
        .map_err(|e| RuTeXError::backend_error(format!("SVG parse error: {}", e)))?;
    
    // Calculate scale based on PPI (assuming 96 is default)
    let scale = ppi / 96.0;
    let width = (rtree.size().width() * scale).round() as u32;
    let height = (rtree.size().height() * scale).round() as u32;
    
    let mut pixmap = tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| RuTeXError::backend_error("Failed to create pixmap"))?;
    
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&rtree, transform, &mut pixmap.as_mut());
    
    pixmap.encode_png()
        .map_err(|e| RuTeXError::backend_error(format!("PNG encoding error: {}", e)))
}


