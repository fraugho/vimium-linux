use crate::atspi::ClickableElement;

/// Element with an assigned hint label
#[derive(Debug, Clone)]
pub struct HintedElement {
    pub hint: String,
    pub element: ClickableElement,
}

impl HintedElement {
    /// Get the x,y position for clicking (center of element)
    pub fn click_position(&self) -> (i32, i32) {
        self.element.center()
    }
}

/// Default characters used for hint labels (home row first for easy typing)
pub const DEFAULT_HINT_CHARS: &str = "asdfghjklqwertyuiopzxcvbnm";

/// Generate hint labels for a given count of elements
/// Returns labels like: a, s, d, ..., aa, as, ad, ...
pub fn generate_hints(count: usize, chars: &str) -> Vec<String> {
    let mut hints = Vec::with_capacity(count);

    if count == 0 {
        return hints;
    }

    let hint_chars: Vec<char> = chars.chars().collect();
    if hint_chars.is_empty() {
        return hints;
    }

    // First pass: single character hints
    for &c in &hint_chars {
        if hints.len() >= count {
            break;
        }
        hints.push(c.to_string());
    }

    // Second pass: two character hints (if needed)
    if hints.len() < count {
        'outer: for &c1 in &hint_chars {
            for &c2 in &hint_chars {
                if hints.len() >= count {
                    break 'outer;
                }
                hints.push(format!("{}{}", c1, c2));
            }
        }
    }

    // Third pass: three character hints (if needed for very large element counts)
    if hints.len() < count {
        'outer: for &c1 in &hint_chars {
            for &c2 in &hint_chars {
                for &c3 in &hint_chars {
                    if hints.len() >= count {
                        break 'outer;
                    }
                    hints.push(format!("{}{}{}", c1, c2, c3));
                }
            }
        }
    }

    hints
}

/// Assign hints to elements using custom characters
pub fn assign_hints(elements: &[ClickableElement], chars: &str) -> Vec<HintedElement> {
    let chars = if chars.is_empty() {
        DEFAULT_HINT_CHARS
    } else {
        chars
    };

    let hints = generate_hints(elements.len(), chars);

    elements
        .iter()
        .zip(hints.into_iter())
        .map(|(element, hint)| HintedElement {
            hint,
            element: element.clone(),
        })
        .collect()
}

/// Filter hinted elements by partial input
/// Returns elements whose hints start with the given prefix
pub fn filter_by_prefix<'a>(
    elements: &'a [HintedElement],
    prefix: &str,
) -> Vec<&'a HintedElement> {
    let prefix_lower = prefix.to_lowercase();
    elements
        .iter()
        .filter(|e| e.hint.starts_with(&prefix_lower))
        .collect()
}

/// Check if exactly one element matches the prefix (for auto-selection)
pub fn find_exact_match<'a>(
    elements: &'a [HintedElement],
    prefix: &str,
) -> Option<&'a HintedElement> {
    let matches: Vec<_> = filter_by_prefix(elements, prefix);
    if matches.len() == 1 && matches[0].hint == prefix.to_lowercase() {
        Some(matches[0])
    } else {
        None
    }
}

/// Check if only one element remains after filtering (for auto-selection)
pub fn find_unique_match<'a>(
    elements: &'a [HintedElement],
    prefix: &str,
) -> Option<&'a HintedElement> {
    let matches: Vec<_> = filter_by_prefix(elements, prefix);
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_element(name: &str) -> ClickableElement {
        ClickableElement {
            name: name.to_string(),
            role: "button".to_string(),
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }
    }

    #[test]
    fn test_generate_hints_single_char() {
        let hints = generate_hints(5, "asdfg");
        assert_eq!(hints, vec!["a", "s", "d", "f", "g"]);
    }

    #[test]
    fn test_generate_hints_default_chars() {
        let hints = generate_hints(5, DEFAULT_HINT_CHARS);
        assert_eq!(hints, vec!["a", "s", "d", "f", "g"]);
    }

    #[test]
    fn test_generate_hints_exceeds_single() {
        let hints = generate_hints(30, DEFAULT_HINT_CHARS);
        assert_eq!(hints.len(), 30);
        // Should start with single chars then move to doubles
        assert_eq!(hints[0], "a");
        assert_eq!(hints[25], "m");
        assert_eq!(hints[26], "aa");
    }

    #[test]
    fn test_generate_hints_custom_chars() {
        let hints = generate_hints(5, "hjkl");
        assert_eq!(hints, vec!["h", "j", "k", "l", "hh"]);
    }

    #[test]
    fn test_filter_by_prefix() {
        let elements = vec![
            make_element("btn1"),
            make_element("btn2"),
            make_element("btn3"),
        ];
        let hinted = assign_hints(&elements, "abc");

        let filtered = filter_by_prefix(&hinted, "a");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].hint, "a");

        let filtered = filter_by_prefix(&hinted, "");
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_find_exact_match() {
        let elements = vec![
            make_element("btn1"),
            make_element("btn2"),
            make_element("btn3"),
        ];
        let hinted = assign_hints(&elements, DEFAULT_HINT_CHARS);

        // Exact match
        assert!(find_exact_match(&hinted, "a").is_some());
        assert!(find_exact_match(&hinted, "s").is_some());

        // Partial match - no exact
        assert!(find_exact_match(&hinted, "").is_none());
    }

    #[test]
    fn test_find_unique_match() {
        let elements = vec![make_element("btn1"), make_element("btn2")];
        let hinted = assign_hints(&elements, "ab");

        // "a" uniquely matches first element
        let m = find_unique_match(&hinted, "a");
        assert!(m.is_some());
        assert_eq!(m.unwrap().hint, "a");
    }
}
