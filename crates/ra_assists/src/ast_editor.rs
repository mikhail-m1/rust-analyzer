use std::{iter, ops::RangeInclusive};

use arrayvec::ArrayVec;
use rustc_hash::FxHashMap;

use ra_fmt::leading_indent;
use ra_syntax::{
    algo,
    ast::{self, make::tokens, TypeBoundsOwner},
    AstNode, Direction, InsertPosition, SyntaxElement,
    SyntaxKind::*,
    T,
};
use ra_text_edit::TextEditBuilder;

pub struct AstEditor<N: AstNode> {
    original_ast: N,
    ast: N,
}

impl<N: AstNode> AstEditor<N> {
    pub fn new(node: N) -> AstEditor<N>
    where
        N: Clone,
    {
        AstEditor { original_ast: node.clone(), ast: node }
    }

    pub fn into_text_edit(self, builder: &mut TextEditBuilder) {
        for (from, to) in algo::diff(&self.original_ast.syntax(), self.ast().syntax()) {
            builder.replace(from.text_range(), to.to_string())
        }
    }

    pub fn ast(&self) -> &N {
        &self.ast
    }

    pub fn replace_descendants<T: AstNode>(
        &mut self,
        replacement_map: impl Iterator<Item = (T, T)>,
    ) -> &mut Self {
        let map = replacement_map
            .map(|(from, to)| (from.syntax().clone().into(), to.syntax().clone().into()))
            .collect::<FxHashMap<_, _>>();
        let new_syntax = algo::replace_descendants(self.ast.syntax(), &map);
        self.ast = N::cast(new_syntax).unwrap();
        self
    }

    #[must_use]
    fn insert_children(
        &self,
        position: InsertPosition<SyntaxElement>,
        mut to_insert: impl Iterator<Item = SyntaxElement>,
    ) -> N {
        let new_syntax = algo::insert_children(self.ast().syntax(), position, &mut to_insert);
        N::cast(new_syntax).unwrap()
    }

    #[must_use]
    fn replace_children(
        &self,
        to_delete: RangeInclusive<SyntaxElement>,
        mut to_insert: impl Iterator<Item = SyntaxElement>,
    ) -> N {
        let new_syntax = algo::replace_children(self.ast().syntax(), to_delete, &mut to_insert);
        N::cast(new_syntax).unwrap()
    }

    fn do_make_multiline(&mut self) {
        let l_curly =
            match self.ast().syntax().children_with_tokens().find(|it| it.kind() == T!['{']) {
                Some(it) => it,
                None => return,
            };
        let sibling = match l_curly.next_sibling_or_token() {
            Some(it) => it,
            None => return,
        };
        let existing_ws = match sibling.as_token() {
            None => None,
            Some(tok) if tok.kind() != WHITESPACE => None,
            Some(ws) => {
                if ws.text().contains('\n') {
                    return;
                }
                Some(ws.clone())
            }
        };

        let indent = leading_indent(self.ast().syntax()).unwrap_or("".into());
        let ws = tokens::WsBuilder::new(&format!("\n{}", indent));
        let to_insert = iter::once(ws.ws().into());
        self.ast = match existing_ws {
            None => self.insert_children(InsertPosition::After(l_curly), to_insert),
            Some(ws) => {
                self.replace_children(RangeInclusive::new(ws.clone().into(), ws.into()), to_insert)
            }
        };
    }
}

impl AstEditor<ast::RecordFieldList> {
    pub fn append_field(&mut self, field: &ast::RecordField) {
        self.insert_field(InsertPosition::Last, field)
    }

    pub fn insert_field(
        &mut self,
        position: InsertPosition<&'_ ast::RecordField>,
        field: &ast::RecordField,
    ) {
        let is_multiline = self.ast().syntax().text().contains_char('\n');
        let ws;
        let space = if is_multiline {
            ws = tokens::WsBuilder::new(&format!(
                "\n{}    ",
                leading_indent(self.ast().syntax()).unwrap_or("".into())
            ));
            ws.ws()
        } else {
            tokens::single_space()
        };

        let mut to_insert: ArrayVec<[SyntaxElement; 4]> = ArrayVec::new();
        to_insert.push(space.into());
        to_insert.push(field.syntax().clone().into());
        to_insert.push(tokens::comma().into());

        macro_rules! after_l_curly {
            () => {{
                let anchor = match self.l_curly() {
                    Some(it) => it,
                    None => return,
                };
                InsertPosition::After(anchor)
            }};
        }

        macro_rules! after_field {
            ($anchor:expr) => {
                if let Some(comma) = $anchor
                    .syntax()
                    .siblings_with_tokens(Direction::Next)
                    .find(|it| it.kind() == T![,])
                {
                    InsertPosition::After(comma)
                } else {
                    to_insert.insert(0, tokens::comma().into());
                    InsertPosition::After($anchor.syntax().clone().into())
                }
            };
        };

        let position = match position {
            InsertPosition::First => after_l_curly!(),
            InsertPosition::Last => {
                if !is_multiline {
                    // don't insert comma before curly
                    to_insert.pop();
                }
                match self.ast().fields().last() {
                    Some(it) => after_field!(it),
                    None => after_l_curly!(),
                }
            }
            InsertPosition::Before(anchor) => {
                InsertPosition::Before(anchor.syntax().clone().into())
            }
            InsertPosition::After(anchor) => after_field!(anchor),
        };

        self.ast = self.insert_children(position, to_insert.iter().cloned());
    }

    fn l_curly(&self) -> Option<SyntaxElement> {
        self.ast().syntax().children_with_tokens().find(|it| it.kind() == T!['{'])
    }
}

impl AstEditor<ast::ItemList> {
    pub fn append_items(&mut self, items: impl Iterator<Item = ast::ImplItem>) {
        if !self.ast().syntax().text().contains_char('\n') {
            self.do_make_multiline();
        }
        items.for_each(|it| self.append_item(it));
    }

    pub fn append_item(&mut self, item: ast::ImplItem) {
        let (indent, position) = match self.ast().impl_items().last() {
            Some(it) => (
                leading_indent(it.syntax()).unwrap_or_default().to_string(),
                InsertPosition::After(it.syntax().clone().into()),
            ),
            None => match self.l_curly() {
                Some(it) => (
                    "    ".to_string() + &leading_indent(self.ast().syntax()).unwrap_or_default(),
                    InsertPosition::After(it),
                ),
                None => return,
            },
        };
        let ws = tokens::WsBuilder::new(&format!("\n{}", indent));
        let to_insert: ArrayVec<[SyntaxElement; 2]> =
            [ws.ws().into(), item.syntax().clone().into()].into();
        self.ast = self.insert_children(position, to_insert.into_iter());
    }

    fn l_curly(&self) -> Option<SyntaxElement> {
        self.ast().syntax().children_with_tokens().find(|it| it.kind() == T!['{'])
    }
}

impl AstEditor<ast::ImplItem> {
    pub fn strip_attrs_and_docs(&mut self) {
        while let Some(start) = self
            .ast()
            .syntax()
            .children_with_tokens()
            .find(|it| it.kind() == ATTR || it.kind() == COMMENT)
        {
            let end = match &start.next_sibling_or_token() {
                Some(el) if el.kind() == WHITESPACE => el.clone(),
                Some(_) | None => start.clone(),
            };
            self.ast = self.replace_children(RangeInclusive::new(start, end), iter::empty());
        }
    }
}

impl AstEditor<ast::TypeParam> {
    pub fn remove_bounds(&mut self) -> &mut Self {
        let colon = match self.ast.colon_token() {
            Some(it) => it,
            None => return self,
        };
        let end = match self.ast.type_bound_list() {
            Some(it) => it.syntax().clone().into(),
            None => colon.clone().into(),
        };
        self.ast = self.replace_children(RangeInclusive::new(colon.into(), end), iter::empty());
        self
    }
}
