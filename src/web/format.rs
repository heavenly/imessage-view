pub fn format_phone_display(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.contains('@')
        || trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
        || !trimmed
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '(' | ')' | '-' | '.' | ' '))
    {
        return trimmed.to_string();
    }

    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() == 10 {
        return format!("({}) {}-{}", &digits[0..3], &digits[3..6], &digits[6..10]);
    }

    if digits.len() == 11 && digits.starts_with('1') {
        return format!(
            "+1 ({}) {}-{}",
            &digits[1..4],
            &digits[4..7],
            &digits[7..11]
        );
    }

    trimmed.to_string()
}

pub fn format_contact_value(value: &str) -> String {
    format_phone_display(value)
}

pub fn format_contact_label(display_name: Option<&str>, handle: Option<&str>) -> String {
    display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| handle.map(format_contact_value))
        .unwrap_or_else(|| "Unknown".to_string())
}

pub fn format_contact_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| format_contact_value(value))
        .collect()
}

pub fn format_group_participant_summary(participants: &[String]) -> String {
    match participants {
        [] => String::new(),
        [first] => first.clone(),
        [first, second] => format!("{first} and {second}"),
        [first, second, rest @ ..] => {
            let other_count = rest.len();
            let others = if other_count == 1 { "other" } else { "others" };
            format!("{first}, {second}, and {other_count} {others}")
        }
    }
}

pub fn format_conversation_name(
    display_name: Option<&str>,
    participant_names: &[String],
) -> String {
    display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| {
            if participant_names.is_empty() {
                "Unknown".to_string()
            } else {
                format_contact_list(participant_names).join(", ")
            }
        })
}

pub fn display_initial(value: &str) -> String {
    value
        .chars()
        .find(|ch| ch.is_ascii_alphanumeric())
        .or_else(|| value.chars().find(|ch| !ch.is_whitespace()))
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| "?".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_phone_display_plain_ten_digit() {
        assert_eq!(format_phone_display("5551234567"), "(555) 123-4567");
    }

    #[test]
    fn test_format_phone_display_with_country_code() {
        assert_eq!(format_phone_display("+1 555-123-4567"), "+1 (555) 123-4567");
    }

    #[test]
    fn test_format_phone_display_already_formatted() {
        assert_eq!(format_phone_display("(555) 123-4567"), "(555) 123-4567");
    }

    #[test]
    fn test_format_phone_display_leaves_email() {
        assert_eq!(format_phone_display("user@example.com"), "user@example.com");
    }

    #[test]
    fn test_format_phone_display_leaves_non_nanp_numbers() {
        assert_eq!(format_phone_display("+44 20 7946 0958"), "+44 20 7946 0958");
    }

    #[test]
    fn test_format_contact_label_prefers_display_name() {
        assert_eq!(
            format_contact_label(Some("Jane Doe"), Some("5551234567")),
            "Jane Doe"
        );
    }

    #[test]
    fn test_format_group_participant_summary_one_name() {
        assert_eq!(
            format_group_participant_summary(&["John".to_string()]),
            "John"
        );
    }

    #[test]
    fn test_format_group_participant_summary_two_names() {
        assert_eq!(
            format_group_participant_summary(&["John".to_string(), "James".to_string()]),
            "John and James"
        );
    }

    #[test]
    fn test_format_group_participant_summary_three_names() {
        assert_eq!(
            format_group_participant_summary(&[
                "John".to_string(),
                "James".to_string(),
                "Jill".to_string()
            ]),
            "John, James, and 1 other"
        );
    }

    #[test]
    fn test_format_group_participant_summary_many_names() {
        assert_eq!(
            format_group_participant_summary(&[
                "John".to_string(),
                "James".to_string(),
                "Jill".to_string(),
                "Jack".to_string(),
                "June".to_string()
            ]),
            "John, James, and 3 others"
        );
    }

    #[test]
    fn test_display_initial_skips_phone_punctuation() {
        assert_eq!(display_initial("(555) 123-4567"), "5");
    }
}
