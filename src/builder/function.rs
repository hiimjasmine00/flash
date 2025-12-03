use std::sync::Arc;

use crate::{html::Html, url::UrlPath};
use clang::Entity;

use super::{
    builder::Builder,
    shared::output_function,
    traits::{ASTEntry, BuildResult, EntityMethods, Entry, NavItem, OutputEntry},
};

pub struct Function<'e> {
    entity: Entity<'e>,
    overload_index: Option<usize>,
}

impl<'e> Function<'e> {
    pub fn new(entity: Entity<'e>) -> Self {
        Self { entity, overload_index: None }
    }

    pub fn add_overload_index(&mut self, index: usize) {
        self.overload_index = Some(index);
    }
}

impl<'e> Entry<'e> for Function<'e> {
    fn name(&self) -> String {
        self.entity
            .get_name()
            .unwrap_or("`Anonymous function`".into())
    }

    fn url(&self) -> UrlPath {
        self.entity
            .rel_docs_url()
            .expect("Unable to get function URL")
            .append_to_last(self.overload_index.map_or("".into(), |i| i.to_string()).as_str())
    }

    fn build(&self, builder: &Builder<'e>) -> BuildResult {
        builder.create_output_for(self)
    }

    fn nav(&self) -> NavItem {
        NavItem::new_link(&self.name(), self.url(), Some(("code", true)), Vec::new())
    }
}

impl<'e> ASTEntry<'e> for Function<'e> {
    fn entity(&self) -> &Entity<'e> {
        &self.entity
    }

    fn category(&self) -> &'static str {
        "function"
    }
}

impl<'e> OutputEntry<'e> for Function<'e> {
    fn output(&self, builder: &Builder<'e>) -> (Arc<String>, Vec<(&'static str, Html)>) {
        (
            builder.config.templates.function.clone(),
            output_function(self, builder),
        )
    }

    fn description(&self, builder: &'e Builder<'e>) -> String {
        self.output_description(builder)
    }
}
