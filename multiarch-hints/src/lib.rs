use lazy_static::lazy_static;
use std::collections::HashMap;

const DEFAULT_VALUE_MULTIARCH_HINT: i32 = 100;

lazy_static! {
    static ref MULTIARCH_HINTS_VALUE: HashMap<&'static str, i32> = {
        let mut map = HashMap::new();
        map.insert("ma-foreign", 20);
        map.insert("file-conflict", 50);
        map.insert("ma-foreign-library", 20);
        map.insert("dep-any", 20);
        map.insert("ma-same", 20);
        map.insert("arch-all", 20);
        map
    };
}

pub fn calculate_value(hints: &[&str]) -> i32 {
    hints
        .iter()
        .map(|hint| *MULTIARCH_HINTS_VALUE.get(hint).unwrap_or(&0))
        .sum::<i32>()
        + DEFAULT_VALUE_MULTIARCH_HINT
}
