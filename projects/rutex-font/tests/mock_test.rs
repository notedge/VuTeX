use rutex_font::{FontMetricsSystem, FontLoader, GlyphMetrics, Result, RuTeXError, Fixed, GlyphKey};
use async_trait::async_trait;
use std::sync::Arc;

struct MockLoader;

#[async_trait]
impl FontLoader for MockLoader {
    async fn load_font_data(&self, _family: &str) -> Result<Arc<Vec<u8>>> {
        // Return an empty vec or some dummy data. 
        // Real tests would need a real font file or a minimal valid OpenType font.
        Err(RuTeXError::font_error("test", "Mock loader does not provide real data"))
    }
}

#[tokio::test]
async fn test_font_system_cache() {
    let loader = Arc::new(MockLoader);
    let system = FontMetricsSystem::new(loader);
    
    let key = GlyphKey {
        char: 'A',
        font_family: Some("test".to_string()),
        style: rutex_types::FontStyle::Normal,
    };
    
    let metrics = GlyphMetrics {
        width: Fixed::from_f64(1.0),
        height: Fixed::from_f64(1.0),
        depth: Fixed::ZERO,
        italic_correction: Fixed::ZERO,
    };
    
    system.insert_metrics(key.clone(), metrics);
    
    let retrieved = system.get_metrics(&key).await.unwrap();
    assert_eq!(retrieved.width, Fixed::from_f64(1.0));
}
