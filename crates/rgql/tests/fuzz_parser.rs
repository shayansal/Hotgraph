use proptest::prelude::*;
use rgql::RgqlParser;

proptest! {
    #[test]
    fn parser_never_panics_for_arbitrary_input(input in "\\PC{0,512}") {
        let result = RgqlParser::parse(&input);
        if let Err(error) = result {
            prop_assert!(error.position <= input.len());
            prop_assert!(!error.message.trim().is_empty());
        }
    }
}
