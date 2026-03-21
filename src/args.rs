use log::trace;

pub fn parse_options<F>(options: String, mut callback: F) -> Result<(), String>
where
    F: FnMut(&str, Option<&str>) -> Result<(), String>,
{
    for option in options.split(',') {
        let mut option = option.splitn(2, '=');
        if let Some(opt) = option.next() && !opt.is_empty() {
            let value = option.next();
            trace!("Running callback on option '{}' with value: {:?}", opt, value);
            callback(opt, value)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::parse_options;

    #[test]
    fn option_parsing() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_multiple() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value,key2,key3=value7".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_handle_empty() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value,,,key2,,key3=value7".to_string();
        expected.insert("key".to_string(), Some("value".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }

    #[test]
    fn option_parsing_eq() {
        // init
        let mut result = HashMap::new();
        let mut expected = HashMap::new();

        // setup
        let options = "key=value=3,key2,key3=value7=a=b=c".to_string();
        expected.insert("key".to_string(), Some("value=3".to_string()));
        expected.insert("key2".to_string(), None);
        expected.insert("key3".to_string(), Some("value7=a=b=c".to_string()));

        // execute
        parse_options(options, |key, value| {
            let _ = result.insert(key.to_string(), value.map(|v| v.to_string()));
            Ok(())
        })
        .unwrap();

        // verify
        assert_eq!(expected, result)
    }
}
