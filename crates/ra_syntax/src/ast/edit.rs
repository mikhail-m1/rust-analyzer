//! This module contains functions for editing syntax trees. As the trees are
//! immutable, all function here return a fresh copy of the tree, instead of
//! doing an in-place modification.

use arrayvec::ArrayVec;
use std::ops::RangeInclusive;

use crate::{
    algo,
    ast::{self, make, AstNode},
    InsertPosition, SyntaxElement,
};

impl ast::FnDef {
    #[must_use]
    pub fn with_body(&self, body: ast::Block) -> ast::FnDef {
        let mut to_insert: ArrayVec<[SyntaxElement; 2]> = ArrayVec::new();
        let old_body_or_semi: SyntaxElement = if let Some(old_body) = self.body() {
            old_body.syntax().clone().into()
        } else if let Some(semi) = self.semicolon_token() {
            to_insert.push(make::tokens::single_space().into());
            semi.into()
        } else {
            to_insert.push(make::tokens::single_space().into());
            to_insert.push(body.syntax().clone().into());
            return insert_children(self, InsertPosition::Last, to_insert.into_iter());
        };
        to_insert.push(body.syntax().clone().into());
        let replace_range = RangeInclusive::new(old_body_or_semi.clone(), old_body_or_semi);
        replace_children(self, replace_range, to_insert.into_iter())
    }
}

#[must_use]
fn insert_children<N: AstNode>(
    parent: &N,
    position: InsertPosition<SyntaxElement>,
    mut to_insert: impl Iterator<Item = SyntaxElement>,
) -> N {
    let new_syntax = algo::insert_children(parent.syntax(), position, &mut to_insert);
    N::cast(new_syntax).unwrap()
}

#[must_use]
fn replace_children<N: AstNode>(
    parent: &N,
    to_replace: RangeInclusive<SyntaxElement>,
    mut to_insert: impl Iterator<Item = SyntaxElement>,
) -> N {
    let new_syntax = algo::replace_children(parent.syntax(), to_replace, &mut to_insert);
    N::cast(new_syntax).unwrap()
}
