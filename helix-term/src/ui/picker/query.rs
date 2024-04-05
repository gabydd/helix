use std::{collections::HashMap, mem, sync::Arc};

#[derive(Debug)]
pub(super) struct PickerQuery {
    column_names: Box<[Arc<str>]>,
    primary_column: usize,
    inner: HashMap<Arc<str>, Arc<str>>,
}

impl PartialEq<HashMap<Arc<str>, Arc<str>>> for PickerQuery {
    fn eq(&self, other: &HashMap<Arc<str>, Arc<str>>) -> bool {
        self.inner.eq(other)
    }
}

impl PickerQuery {
    pub(super) fn new<I: Iterator<Item = Arc<str>>>(
        column_names: I,
        primary_column: usize,
    ) -> Self {
        let column_names: Box<[_]> = column_names.collect();
        let inner = HashMap::with_capacity(column_names.len());
        Self {
            column_names,
            primary_column,
            inner,
        }
    }

    pub(super) fn get(&self, column: &str) -> Option<&Arc<str>> {
        self.inner.get(column)
    }

    pub(super) fn parse(&mut self, input: &str) -> HashMap<Arc<str>, Arc<str>> {
        let mut fields: HashMap<Arc<str>, String> = HashMap::new();
        let primary_field = &self.column_names[self.primary_column];
        let mut escaped = false;
        let mut quoted = false;
        let mut in_field = false;
        let mut field = None;
        let mut text = String::new();

        macro_rules! finish_field {
            () => {
                let key = field.take().unwrap_or(primary_field);

                if let Some(pattern) = fields.get_mut(key) {
                    pattern.push(' ');
                    pattern.push_str(&text);
                    text.clear();
                } else {
                    fields.insert(key.clone(), std::mem::take(&mut text));
                }
            };
        }

        for ch in input.chars() {
            match ch {
                // Backslash escaping
                _ if escaped => {
                    // Allow escaping '%' and '"'
                    if !matches!(ch, '%' | '"') {
                        text.push('\\');
                    }
                    text.push(ch);
                    escaped = false;
                }
                '\\' => escaped = !escaped,
                // Double quoting
                '"' => quoted = !quoted,
                '%' | ':' | ' ' if quoted => text.push(ch),
                // Space either completes the current word if no field is specified
                // or field if one is specified.
                '%' | ' ' if !text.is_empty() => {
                    finish_field!();
                    in_field = ch == '%';
                }
                '%' => in_field = true,
                ':' if in_field => {
                    // Go over all columns and their indices, find all that starts with field key,
                    // select a column that fits key the most.
                    field = self
                        .column_names
                        .iter()
                        .filter(|col| col.starts_with(&text))
                        // select "fittest" column
                        .min_by_key(|col| col.len());
                    text.clear();
                    in_field = false;
                }
                _ => text.push(ch),
            }
        }

        if !in_field && !text.is_empty() {
            finish_field!();
        }

        let new_inner: HashMap<_, _> = fields
            .into_iter()
            .map(|(field, query)| (field, query.as_str().into()))
            .collect();

        mem::replace(&mut self.inner, new_inner)
    }
}

#[cfg(test)]
mod test {
    use helix_core::hashmap;

    use super::*;

    #[test]
    fn parse_query_test() {
        let mut query = PickerQuery::new(
            [
                "primary".into(),
                "field1".into(),
                "field2".into(),
                "another".into(),
                "anode".into(),
            ]
            .into_iter(),
            0,
        );

        // Basic field splitting
        query.parse("hello world");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello world".into(),
            )
        );
        query.parse("hello %field1:world %field2:!");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "field1".into() => "world".into(),
                "field2".into() => "!".into(),
            )
        );
        query.parse("%field1:abc %field2:def xyz");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "xyz".into(),
                "field1".into() => "abc".into(),
                "field2".into() => "def".into(),
            )
        );

        // Trailing space is trimmed
        query.parse("hello ");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
            )
        );

        // Trailing fields are trimmed.
        query.parse("hello %foo");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
            )
        );

        // Quoting
        query.parse(r#"hello %field1:"a b c""#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "field1".into() => "a b c".into(),
            )
        );

        // Escaping
        query.parse(r#"hello\ world"#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => r#"hello\ world"#.into(),
            )
        );
        query.parse(r#"hello \%field1:world"#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello %field1:world".into(),
            )
        );
        query.parse(r#"hello %field1:"a\"b""#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "field1".into() => r#"a"b"#.into(),
            )
        );
        query.parse(r#"%field1:hello\ world"#);
        assert_eq!(
            query,
            hashmap!(
                "field1".into() => r#"hello\ world"#.into(),
            )
        );
        query.parse(r#"%field1:"hello\ world""#);
        assert_eq!(
            query,
            hashmap!(
                "field1".into() => r#"hello\ world"#.into(),
            )
        );
        query.parse(r#"\bfoo\b"#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => r#"\bfoo\b"#.into(),
            )
        );
        query.parse(r#"\\n"#);
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => r#"\\n"#.into(),
            )
        );

        // Prefix
        query.parse("hello %anot:abc");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "another".into() => "abc".into(),
            )
        );
        query.parse("hello %ano:abc");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "anode".into() => "abc".into()
            )
        );
        query.parse("hello %field1:xyz %fie:abc");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "field1".into() => "xyz abc".into()
            )
        );
        query.parse("hello %fie:abc");
        assert_eq!(
            query,
            hashmap!(
                "primary".into() => "hello".into(),
                "field1".into() => "abc".into()
            )
        );
    }
}
