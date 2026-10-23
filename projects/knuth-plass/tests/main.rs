use rutex_knuth_plass::{Item, KnuthPlass};
use rutex_types::Fixed;

#[test]
fn test_simple_line_break() {
    let items = vec![
        Item::Box { width: Fixed::from_f64(40.0), debug_info: Some("word1".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(5.0), shrink: Fixed::from_f64(2.0) },
        Item::Box { width: Fixed::from_f64(40.0), debug_info: Some("word2".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(5.0), shrink: Fixed::from_f64(2.0) },
        Item::Box { width: Fixed::from_f64(40.0), debug_info: Some("word3".into()) },
    ];

    let kp = KnuthPlass::new(vec![Fixed::from_f64(100.0)], 10.0);
    let breaks = kp.find_breaks(&items);

    assert!(!breaks.is_empty());
    println!("Breaks: {:?}", breaks);
}

#[test]
fn test_exact_fit() {
    let items = vec![
        Item::Box { width: Fixed::from_f64(50.0), debug_info: None },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(0.0), shrink: Fixed::from_f64(0.0) },
        Item::Box { width: Fixed::from_f64(40.0), debug_info: None },
    ];

    let kp = KnuthPlass::new(vec![Fixed::from_f64(100.0)], 1.0);
    let breaks = kp.find_breaks(&items);
    
    println!("Exact fit breaks: {:?}", breaks);
}

#[test]
fn test_multiple_lines() {
    let items = vec![
        Item::Box { width: Fixed::from_f64(30.0), debug_info: Some("1".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(2.0), shrink: Fixed::from_f64(1.0) },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: Some("2".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(2.0), shrink: Fixed::from_f64(1.0) },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: Some("3".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(2.0), shrink: Fixed::from_f64(1.0) },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: Some("4".into()) },
        Item::Glue { width: Fixed::from_f64(10.0), stretch: Fixed::from_f64(2.0), shrink: Fixed::from_f64(1.0) },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: Some("5".into()) },
    ];

    let kp = KnuthPlass::new(vec![Fixed::from_f64(50.0)], 10.0);
    let breaks = kp.find_breaks(&items);
    println!("Multiple lines breaks: {:?}", breaks);
    assert!(breaks.len() >= 2);
}

#[test]
fn test_flagged_penalty() {
    let items = vec![
        Item::Box { width: Fixed::from_f64(30.0), debug_info: None },
        Item::Glue { width: Fixed::from_f64(5.0), stretch: Fixed::from_f64(10.0), shrink: Fixed::ZERO },
        Item::Penalty { width: Fixed::ZERO, penalty: 50.0, flagged: true },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: None },
        Item::Glue { width: Fixed::from_f64(5.0), stretch: Fixed::from_f64(10.0), shrink: Fixed::ZERO },
        Item::Penalty { width: Fixed::ZERO, penalty: 50.0, flagged: true },
        Item::Box { width: Fixed::from_f64(30.0), debug_info: None },
    ];

    // target width 40.
    // Line 1: Box(30) + Glue(5) = 35. diff = 5. stretch = 10. ratio = 0.5. Badness = 12.5.
    let kp = KnuthPlass::new(vec![Fixed::from_f64(40.0)], 20.0);
    let breaks = kp.find_breaks(&items);
    println!("Flagged penalty breaks: {:?}", breaks);
    assert!(!breaks.is_empty());
}
