use regex::{Captures, Regex};

pub struct Template {
    pattern: Regex,
    text: String,
}

impl Template {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            pattern: Regex::new(r"\{\{([a-z_]+)\}\}").unwrap(),
        }
    }

    pub fn set_text<S: Into<String>>(&mut self, text: S) {
        self.text = text.into();
    }

    pub fn render<T: Lookup>(&self, data: &T) -> String {
        self.pattern
            .replace_all(&self.text, |cap: &Captures| data.field(&cap[1]).to_string())
            .into_owned()
    }
}

pub trait Lookup {
    fn field(&self, key: &str) -> &dyn std::fmt::Display;
}

#[cfg(test)]
mod tests {
    struct A;

    impl super::Lookup for A {
        fn field(&self, key: &str) -> &dyn std::fmt::Display {
            match key {
                "a" => &"Hello",
                "b" => &"World",
                _ => &"",
            }
        }
    }

    #[test]
    fn test_template() {
        let mut template = super::Template::new();
        template.set_text("{{a}}, {{b}}!");
        assert_eq!(template.render(&A), "Hello, World!");
    }
}
