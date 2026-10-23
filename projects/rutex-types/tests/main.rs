use rutex_types::{Fixed, RuTeXError};

#[test]
fn test_fixed_math() {
    let a = Fixed::from_f64(1.5);
    let b = Fixed::from_f64(2.0);
    
    let sum = a + b;
    assert_eq!(sum.to_f64(), 3.5);
    
    let diff = b - a;
    assert_eq!(diff.to_f64(), 0.5);
    
    let prod = a * b;
    assert_eq!(prod.to_f64(), 3.0);
    
    let div = b / a;
    assert!((div.to_f64() - 1.3333).abs() < 0.001);
}

#[test]
fn test_error_display() {
    let err = RuTeXError::parse_error("Unexpected character", Some(10));
    assert_eq!(format!("{}", err), "Parse Error at position 10: Unexpected character");
}
