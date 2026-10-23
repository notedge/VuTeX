use rutex_parser::{parse, Parser, macro_system::MacroDefinition, lexer::Token};
use rutex_types::SemanticNode;

#[test]
fn test_def_command() {
    let input = "\\def\\plus{+} 1 \\plus 2";
    let tree = parse(input).unwrap();
    
    if let SemanticNode::Sequence(nodes) = tree.root {
        // \def produces an empty sequence, then 1, then +, then 2
        assert!(nodes.len() >= 3);
        let plus_node = nodes.iter().find(|n| {
            if let SemanticNode::Symbol { glyph_key, .. } = n {
                glyph_key.char == '+'
            } else {
                false
            }
        });
        assert!(plus_node.is_some());
    } else {
        panic!("Expected sequence");
    }
}

#[test]
fn test_macro_expansion() {
    use rutex_parser::ParseContext;
    use rutex_parser::macro_system::ParseEffect;

    let def = MacroDefinition {
        name: "double".to_string(),
        args_count: 1,
        body: vec![Token::Parameter(1), Token::Parameter(1)],
    };
    
    let mut ctx = ParseContext::default();
    ctx = ctx.apply_effect(ParseEffect::DefineMacro(def)).unwrap();

    let mut parser = Parser::with_context("\\double{x}", ctx);
    let tree = parser.parse().unwrap();

    if let SemanticNode::Sequence(nodes) = tree.root {
        assert_eq!(nodes.len(), 2);
        match (&nodes[0], &nodes[1]) {
            (SemanticNode::Symbol { .. }, SemanticNode::Symbol { .. }) => {},
            _ => panic!("Expected two symbols, got {:?}", nodes),
        }
    } else {
        panic!("Expected sequence, got {:?}", tree.root);
    }
}

#[test]
fn test_basic_arithmetic() {
    let input = "a + b = c";
    let tree = parse(input).unwrap();
    
    if let SemanticNode::Sequence(nodes) = tree.root {
        assert_eq!(nodes.len(), 5);
    } else {
        panic!("Expected sequence");
    }
}

#[test]
fn test_fraction() {
    let input = "\\frac{1}{2}";
    let tree = parse(input).unwrap();
    
    match tree.root {
        SemanticNode::Fraction { .. } => {},
        _ => panic!("Expected fraction, got {:?}", tree.root),
    }
}

#[test]
fn test_sub_sup() {
    let input = "x^2 + y_1";
    let tree = parse(input).unwrap();
    
    if let SemanticNode::Sequence(nodes) = tree.root {
        assert_eq!(nodes.len(), 3);
        match &nodes[0] {
            SemanticNode::Superscript { .. } => {},
            _ => panic!("Expected superscript"),
        }
        match &nodes[2] {
            SemanticNode::Subscript { .. } => {},
            _ => panic!("Expected subscript"),
        }
    } else {
        panic!("Expected sequence");
    }
}

#[test]
fn test_matrix() {
    let input = "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}";
    let tree = parse(input).unwrap();
    
    match tree.root {
        SemanticNode::Delimited { left, right, content } => {
            assert_eq!(left.unwrap().char, '(');
            assert_eq!(right.unwrap().char, ')');
            if let SemanticNode::Matrix { rows, .. } = *content {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[1].len(), 2);
            } else {
                panic!("Expected matrix inside delimiters, got {:?}", content);
            }
        }
        _ => panic!("Expected delimited node, got {:?}", tree.root),
    }
}

#[test]
fn test_sqrt() {
    let input = "\\sqrt{x} + \\sqrt[3]{y}";
    let tree = parse(input).unwrap();
    
    if let SemanticNode::Sequence(nodes) = tree.root {
        assert_eq!(nodes.len(), 3);
        match &nodes[0] {
            SemanticNode::Radical { degree, .. } => assert!(degree.is_none()),
            _ => panic!("Expected radical"),
        }
        match &nodes[2] {
            SemanticNode::Radical { degree, .. } => assert!(degree.is_some()),
            _ => panic!("Expected radical with degree"),
        }
    }
}
