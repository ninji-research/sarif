use crate::{Diagnostic, Element, Node, NodeKind, Token, TokenKind};

#[derive(Clone, Debug)]
pub struct ParseOutput {
    pub root: Node,
    pub diagnostics: Vec<Diagnostic>,
}

#[must_use]
pub fn parse(tokens: &[Token]) -> ParseOutput {
    Parser::new(tokens).parse()
}

struct Parser<'a> {
    tokens: &'a [Token],
    cursor: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    const fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            cursor: 0,
            diagnostics: Vec::new(),
        }
    }

    fn parse(mut self) -> ParseOutput {
        let mut children = Vec::new();

        self.collect_trivia(&mut children);

        // 1. Types (enum, struct, effect)
        while self.at(TokenKind::KwEnum)
            || self.at(TokenKind::KwStruct)
            || self.at(TokenKind::KwEffect)
        {
            if self.at(TokenKind::KwEnum) {
                children.push(Element::Node(self.parse_enum_item()));
            } else if self.at(TokenKind::KwStruct) {
                children.push(Element::Node(self.parse_struct_item()));
            } else {
                children.push(Element::Node(self.parse_effect_item()));
            }
            self.collect_trivia(&mut children);
        }

        // 2. Constants
        while self.at(TokenKind::KwConst) {
            children.push(Element::Node(self.parse_const_item()));
            self.collect_trivia(&mut children);
        }

        // 3. Functions
        while self.at(TokenKind::KwFn) {
            children.push(Element::Node(self.parse_fn_item()));
            self.collect_trivia(&mut children);
        }

        if !self.at(TokenKind::Eof) {
            let token = self.bump();
            self.diagnostics.push(Diagnostic::new(
                "parse.out-of-order-item",
                format!("unexpected token `{:?}`: top-level items must follow the order: Types (enum, struct) -> Consts -> Functions", token.kind),
                token.span,
                Some("Move this item to its canonical section.".to_owned()),
            ));
            children.push(Element::Token(token));
        }

        if self.at(TokenKind::Eof) {
            children.push(Element::Token(self.bump()));
        }

        ParseOutput {
            root: Node::new(NodeKind::SourceFile, children),
            diagnostics: self.diagnostics,
        }
    }

    fn parse_const_item(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwConst)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Colon)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_type()));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Eq)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Semicolon)));
        Node::new(NodeKind::ConstItem, children)
    }

    fn parse_fn_item(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwFn)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);

        if self.at(TokenKind::LBracket) {
            children.push(Element::Node(self.parse_generic_params()));
            self.collect_trivia(&mut children);
        }

        children.push(Element::Token(self.expect(TokenKind::LParen)));
        children.push(Element::Node(self.parse_param_list()));
        children.push(Element::Token(self.expect(TokenKind::RParen)));

        self.collect_trivia(&mut children);
        if self.at(TokenKind::Arrow) {
            let mut return_children = Vec::new();
            return_children.push(Element::Token(self.bump()));
            self.collect_trivia(&mut return_children);
            return_children.push(Element::Node(self.parse_type()));
            children.push(Element::Node(Node::new(
                NodeKind::ReturnType,
                return_children,
            )));
        }

        self.collect_trivia(&mut children);
        if self.at(TokenKind::KwEffects) {
            children.push(Element::Node(self.parse_effects_clause()));
        }

        self.collect_trivia(&mut children);
        if self.at(TokenKind::KwRequires) {
            children.push(Element::Node(self.parse_contract_clause(
                TokenKind::KwRequires,
                NodeKind::RequiresClause,
            )));
        }

        self.collect_trivia(&mut children);
        if self.at(TokenKind::KwEnsures) {
            children.push(Element::Node(
                self.parse_contract_clause(TokenKind::KwEnsures, NodeKind::EnsuresClause),
            ));
        }

        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));

        Node::new(NodeKind::FnItem, children)
    }

    fn parse_enum_item(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwEnum)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        children.push(Element::Node(self.parse_variant_list()));
        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::EnumItem, children)
    }

    fn parse_struct_item(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwStruct)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        children.push(Element::Node(self.parse_field_list()));
        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::StructItem, children)
    }

    fn parse_generic_params(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::LBracket)));
        self.collect_trivia(&mut children);

        if !self.at(TokenKind::RBracket) {
            loop {
                let mut param_children = Vec::new();
                param_children.push(Element::Token(self.expect(TokenKind::Ident)));
                self.collect_trivia(&mut param_children);
                if self.at(TokenKind::Colon) {
                    param_children.push(Element::Token(self.bump()));
                    self.collect_trivia(&mut param_children);
                    param_children.push(Element::Token(self.expect(TokenKind::Ident)));
                    self.collect_trivia(&mut param_children);
                }
                children.push(Element::Node(Node::new(
                    NodeKind::GenericParam,
                    param_children,
                )));

                if self.at(TokenKind::Comma) {
                    children.push(Element::Token(self.bump()));
                    self.collect_trivia(&mut children);
                } else {
                    break;
                }
            }
        }

        children.push(Element::Token(self.expect(TokenKind::RBracket)));
        Node::new(NodeKind::GenericParamList, children)
    }

    fn parse_param_list(&mut self) -> Node {
        let mut children = Vec::new();
        let offset = self.current().span.start;

        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RParen) || self.at(TokenKind::Eof) {
                break;
            }

            children.push(Element::Node(self.parse_param()));
            self.collect_trivia(&mut children);

            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if children.is_empty() {
            Node::empty(NodeKind::ParamList, offset)
        } else {
            Node::new(NodeKind::ParamList, children)
        }
    }

    fn parse_param(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Colon)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_type()));
        Node::new(NodeKind::Param, children)
    }

    fn parse_effects_clause(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwEffects)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBracket)));

        let mut effects = Vec::new();
        let offset = self.current().span.start;
        while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut effects);

            if self.at(TokenKind::RBracket) || self.at(TokenKind::Eof) {
                break;
            }

            effects.push(Element::Token(self.expect(TokenKind::Ident)));
            self.collect_trivia(&mut effects);

            if self.at(TokenKind::Comma) {
                effects.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if effects.is_empty() {
            children.push(Element::Node(Node::empty(NodeKind::EffectList, offset)));
        } else {
            children.push(Element::Node(Node::new(NodeKind::EffectList, effects)));
        }
        children.push(Element::Token(self.expect(TokenKind::RBracket)));
        Node::new(NodeKind::EffectsClause, children)
    }

    fn parse_field_list(&mut self) -> Node {
        let mut children = Vec::new();
        let offset = self.current().span.start;

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RBrace) || self.at(TokenKind::Eof) {
                break;
            }

            let mut field_children = Vec::new();
            field_children.push(Element::Token(self.expect(TokenKind::Ident)));
            self.collect_trivia(&mut field_children);
            field_children.push(Element::Token(self.expect(TokenKind::Colon)));
            self.collect_trivia(&mut field_children);
            field_children.push(Element::Node(self.parse_type()));
            children.push(Element::Node(Node::new(NodeKind::Field, field_children)));

            self.collect_trivia(&mut children);
            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if children.is_empty() {
            Node::empty(NodeKind::FieldList, offset)
        } else {
            Node::new(NodeKind::FieldList, children)
        }
    }

    fn parse_variant_list(&mut self) -> Node {
        let mut children = Vec::new();
        let offset = self.current().span.start;

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RBrace) || self.at(TokenKind::Eof) {
                break;
            }

            let mut variant_children = vec![Element::Token(self.expect(TokenKind::Ident))];
            self.collect_trivia(&mut variant_children);
            if self.at(TokenKind::LParen) {
                variant_children.push(Element::Token(self.bump()));
                self.collect_trivia(&mut variant_children);
                variant_children.push(Element::Node(self.parse_type()));
                self.collect_trivia(&mut variant_children);
                variant_children.push(Element::Token(self.expect(TokenKind::RParen)));
            }
            children.push(Element::Node(Node::new(
                NodeKind::Variant,
                variant_children,
            )));
            self.collect_trivia(&mut children);

            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if children.is_empty() {
            Node::empty(NodeKind::VariantList, offset)
        } else {
            Node::new(NodeKind::VariantList, children)
        }
    }

    fn parse_contract_clause(&mut self, keyword: TokenKind, kind: NodeKind) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(keyword)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        Node::new(kind, children)
    }

    fn parse_type(&mut self) -> Node {
        if self.at(TokenKind::LBracket) {
            self.parse_array_type()
        } else {
            self.parse_type_path()
        }
    }

    fn parse_type_path(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::Ident)));

        while self.peek_trivia_then(TokenKind::Dot) {
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(TokenKind::Dot)));
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(TokenKind::Ident)));
        }

        Node::new(NodeKind::TypePath, children)
    }

    fn parse_array_type(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::LBracket)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_type()));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Semicolon)));
        self.collect_trivia(&mut children);
        match self.current_non_trivia_kind() {
            Some(TokenKind::Integer) => {
                children.push(Element::Token(self.expect(TokenKind::Integer)));
            }
            Some(TokenKind::Ident) => children.push(Element::Token(self.expect(TokenKind::Ident))),
            _ => children.push(Element::Token(self.expect(TokenKind::Integer))),
        }
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::RBracket)));
        Node::new(NodeKind::TypeArray, children)
    }

    fn parse_comptime_expr(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwComptime)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        Node::new(NodeKind::ExprComptime, children)
    }

    fn parse_effect_item(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwEffect)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        self.collect_trivia(&mut children);
        while self.at(TokenKind::KwFn) {
            let mut method_children = Vec::new();
            method_children.push(Element::Token(self.bump()));
            self.collect_trivia(&mut method_children);
            method_children.push(Element::Token(self.expect(TokenKind::Ident)));
            self.collect_trivia(&mut method_children);
            method_children.push(Element::Token(self.expect(TokenKind::LParen)));
            method_children.push(Element::Node(self.parse_param_list()));
            method_children.push(Element::Token(self.expect(TokenKind::RParen)));
            self.collect_trivia(&mut method_children);
            if self.at(TokenKind::Arrow) {
                let mut return_children = Vec::new();
                return_children.push(Element::Token(self.bump()));
                self.collect_trivia(&mut return_children);
                return_children.push(Element::Node(self.parse_type()));
                method_children.push(Element::Node(Node::new(
                    NodeKind::ReturnType,
                    return_children,
                )));
            }
            self.collect_trivia(&mut method_children);
            method_children.push(Element::Token(self.expect(TokenKind::Semicolon)));
            children.push(Element::Node(Node::new(NodeKind::FnItem, method_children)));
            self.collect_trivia(&mut children);
        }
        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::EffectItem, children)
    }

    fn parse_handle_expr(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwHandle)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::KwWith)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        self.collect_trivia(&mut children);
        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            let mut arm_children = Vec::new();
            arm_children.push(Element::Token(self.expect(TokenKind::Ident)));
            self.collect_trivia(&mut arm_children);
            arm_children.push(Element::Token(self.expect(TokenKind::LParen)));
            self.collect_trivia(&mut arm_children);
            while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
                arm_children.push(Element::Token(self.expect(TokenKind::Ident)));
                self.collect_trivia(&mut arm_children);
                if self.at(TokenKind::Comma) {
                    arm_children.push(Element::Token(self.bump()));
                    self.collect_trivia(&mut arm_children);
                }
            }
            arm_children.push(Element::Token(self.expect(TokenKind::RParen)));
            self.collect_trivia(&mut arm_children);
            arm_children.push(Element::Token(self.expect(TokenKind::FatArrow)));
            self.collect_trivia(&mut arm_children);
            arm_children.push(Element::Node(self.parse_expr_body()));
            children.push(Element::Node(Node::new(NodeKind::HandleArm, arm_children)));
            self.collect_trivia(&mut children);
        }
        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::ExprHandle, children)
    }

    fn parse_expr_body(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        self.collect_trivia(&mut children);

        loop {
            let statement = match self.current_non_trivia_kind() {
                Some(TokenKind::KwLet) => Some(self.parse_let_stmt()),
                Some(TokenKind::Ident) if self.next_name_is_assign_statement() => {
                    Some(self.parse_assign_stmt())
                }
                Some(kind) if Self::starts_expr(kind) && self.next_expr_is_statement() => {
                    Some(self.parse_expr_stmt())
                }
                _ => None,
            };

            let Some(statement) = statement else {
                break;
            };
            children.push(Element::Node(statement));
            self.collect_trivia(&mut children);
        }

        if !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            children.push(Element::Node(self.parse_expr_bp(0)));
            self.collect_trivia(&mut children);

            while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
                let token = self.bump();
                self.diagnostics.push(Diagnostic::new(
                    "parse.unexpected-token",
                    format!("unexpected token `{:?}` in function body", token.kind),
                    token.span,
                    Some(
                        "This stage-0 body grammar accepts zero or more `let`/assignment/unit-expression statements followed by one optional tail expression.".to_owned(),
                    ),
                ));
                children.push(Element::Token(token));
                self.collect_trivia(&mut children);
            }
        }

        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::Body, children)
    }

    fn next_expr_is_statement(&self) -> bool {
        let mut probe = Parser {
            tokens: self.tokens,
            cursor: self.cursor,
            diagnostics: Vec::new(),
        };
        let _ = probe.parse_expr_bp(0);
        let mut trivia = Vec::new();
        probe.collect_trivia(&mut trivia);
        probe.at(TokenKind::Semicolon)
    }

    fn next_name_is_assign_statement(&self) -> bool {
        let mut index = self.cursor;

        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }

        if !self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Ident)
        {
            return false;
        }
        index += 1;

        loop {
            while self
                .tokens
                .get(index)
                .is_some_and(|token| token.kind.is_trivia())
            {
                index += 1;
            }

            if !self
                .tokens
                .get(index)
                .is_some_and(|token| token.kind == TokenKind::LBracket)
            {
                break;
            }

            let mut depth = 0usize;
            while let Some(token) = self.tokens.get(index) {
                match token.kind {
                    TokenKind::LBracket => depth += 1,
                    TokenKind::RBracket => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            index += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }

            if depth != 0 {
                return false;
            }
        }

        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Eq)
    }

    const fn starts_expr(kind: TokenKind) -> bool {
        matches!(
            kind,
            TokenKind::Integer
                | TokenKind::Float
                | TokenKind::Minus
                | TokenKind::String
                | TokenKind::KwTrue
                | TokenKind::KwFalse
                | TokenKind::Ident
                | TokenKind::LBracket
                | TokenKind::LParen
                | TokenKind::KwIf
                | TokenKind::KwMatch
                | TokenKind::KwRepeat
                | TokenKind::KwWhile
                | TokenKind::KwComptime
                | TokenKind::KwHandle
        )
    }

    fn parse_let_stmt(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Token(self.expect(TokenKind::KwLet)));
        self.collect_trivia(&mut children);
        if self.at(TokenKind::KwMut) {
            children.push(Element::Token(self.bump()));
            self.collect_trivia(&mut children);
        }
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Eq)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Semicolon)));
        Node::new(NodeKind::LetStmt, children)
    }

    fn parse_assign_stmt(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Node(self.parse_assign_target_expr()));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Eq)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Semicolon)));
        Node::new(NodeKind::AssignStmt, children)
    }

    fn parse_assign_target_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Ident)));
        let mut expr = Node::new(NodeKind::ExprName, children);

        loop {
            if self.peek_trivia_then(TokenKind::LBracket) {
                let mut index_children = vec![Element::Node(expr)];
                self.collect_trivia(&mut index_children);
                index_children.push(Element::Token(self.expect(TokenKind::LBracket)));
                self.collect_trivia(&mut index_children);
                index_children.push(Element::Node(self.parse_expr_bp(0)));
                self.collect_trivia(&mut index_children);
                index_children.push(Element::Token(self.expect(TokenKind::RBracket)));
                expr = Node::new(NodeKind::ExprIndex, index_children);
                continue;
            }
            break;
        }

        expr
    }

    fn parse_expr_stmt(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::Semicolon)));
        Node::new(NodeKind::ExprStmt, children)
    }

    fn parse_expr_bp(&mut self, min_bp: u8) -> Node {
        let mut lhs = self.parse_postfix_expr();

        loop {
            let Some(op_kind) = self.peek_non_trivia_kind() else {
                break;
            };
            let Some((left_bp, right_bp)) = infix_binding_power(op_kind) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }

            let mut children = vec![Element::Node(lhs)];
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(op_kind)));
            self.collect_trivia(&mut children);
            children.push(Element::Node(self.parse_expr_bp(right_bp)));
            lhs = Node::new(NodeKind::ExprBinary, children);
        }

        lhs
    }

    fn parse_postfix_expr(&mut self) -> Node {
        let mut expr = self.parse_prefix_expr();

        loop {
            if self.peek_trivia_then(TokenKind::LParen) {
                let mut children = vec![Element::Node(expr)];
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::LParen)));
                children.push(Element::Node(self.parse_arg_list()));
                children.push(Element::Token(self.expect(TokenKind::RParen)));
                expr = Node::new(NodeKind::ExprCall, children);
                continue;
            }

            if self.peek_trivia_then(TokenKind::Dot) {
                let mut children = vec![Element::Node(expr)];
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::Dot)));
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::Ident)));
                expr = Node::new(NodeKind::ExprField, children);
                continue;
            }

            if self.peek_trivia_then(TokenKind::LBracket) {
                let mut children = vec![Element::Node(expr)];
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::LBracket)));
                self.collect_trivia(&mut children);
                children.push(Element::Node(self.parse_expr_bp(0)));
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::RBracket)));
                expr = Node::new(NodeKind::ExprIndex, children);
                continue;
            }

            break;
        }

        expr
    }

    fn parse_prefix_expr(&mut self) -> Node {
        if matches!(self.current_non_trivia_kind(), Some(TokenKind::KwNot)) {
            let mut children = Vec::new();
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(TokenKind::KwNot)));
            self.collect_trivia(&mut children);
            children.push(Element::Node(self.parse_prefix_expr()));
            return Node::new(NodeKind::ExprUnary, children);
        }

        self.parse_primary_expr()
    }

    fn parse_primary_expr(&mut self) -> Node {
        match self.current_non_trivia_kind() {
            Some(TokenKind::KwIf) => self.parse_if_expr(),
            Some(TokenKind::KwMatch) => self.parse_match_expr(),
            Some(TokenKind::KwRepeat) => self.parse_repeat_expr(),
            Some(TokenKind::KwWhile) => self.parse_while_expr(),
            Some(TokenKind::KwComptime) => self.parse_comptime_expr(),
            Some(TokenKind::KwHandle) => self.parse_handle_expr(),
            Some(TokenKind::LBracket) => self.parse_array_expr(),
            Some(TokenKind::Integer) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::Integer)));
                Node::new(NodeKind::ExprInteger, children)
            }
            Some(TokenKind::Float) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::Float)));
                Node::new(NodeKind::ExprFloat, children)
            }
            Some(TokenKind::Minus) if self.next_non_trivia_kind_is_numeric_literal() => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::Minus)));
                self.collect_trivia(&mut children);
                match self.current_non_trivia_kind() {
                    Some(TokenKind::Integer) => {
                        children.push(Element::Token(self.expect(TokenKind::Integer)));
                        Node::new(NodeKind::ExprInteger, children)
                    }
                    Some(TokenKind::Float) => {
                        children.push(Element::Token(self.expect(TokenKind::Float)));
                        Node::new(NodeKind::ExprFloat, children)
                    }
                    _ => self.parse_error_expr(),
                }
            }
            Some(TokenKind::String) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::String)));
                Node::new(NodeKind::ExprString, children)
            }
            Some(TokenKind::KwTrue | TokenKind::KwFalse) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                let kind = self.current_non_trivia_kind().expect("bool token present");
                children.push(Element::Token(self.expect(kind)));
                Node::new(NodeKind::ExprBool, children)
            }
            Some(TokenKind::Ident) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                let token = self.expect(TokenKind::Ident);
                let lexeme = token.lexeme.clone();
                children.push(Element::Token(token));
                if lexeme == "result" {
                    Node::new(NodeKind::ExprContractResult, children)
                } else if self.starts_record_literal() {
                    self.collect_trivia(&mut children);
                    children.push(Element::Token(self.expect(TokenKind::LBrace)));
                    children.push(Element::Node(self.parse_field_init_list()));
                    children.push(Element::Token(self.expect(TokenKind::RBrace)));
                    Node::new(NodeKind::ExprRecord, children)
                } else {
                    Node::new(NodeKind::ExprName, children)
                }
            }
            Some(TokenKind::LParen) => {
                let mut children = Vec::new();
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::LParen)));
                self.collect_trivia(&mut children);
                children.push(Element::Node(self.parse_expr_bp(0)));
                self.collect_trivia(&mut children);
                children.push(Element::Token(self.expect(TokenKind::RParen)));
                Node::new(NodeKind::ExprGroup, children)
            }
            Some(_) => self.parse_error_expr(),
            None => Node::empty(NodeKind::Error, self.current().span.start),
        }
    }

    fn parse_if_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::KwIf)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        if self.peek_trivia_then(TokenKind::KwElse) {
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(TokenKind::KwElse)));
            self.collect_trivia(&mut children);
            children.push(Element::Node(self.parse_expr_body()));
        }
        Node::new(NodeKind::ExprIf, children)
    }

    fn parse_match_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::KwMatch)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBrace)));
        self.collect_trivia(&mut children);

        while self.starts_match_pattern() {
            children.push(Element::Node(self.parse_match_arm()));
            self.collect_trivia(&mut children);
            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
                self.collect_trivia(&mut children);
            } else {
                break;
            }
        }

        children.push(Element::Token(self.expect(TokenKind::RBrace)));
        Node::new(NodeKind::ExprMatch, children)
    }

    fn parse_match_arm(&mut self) -> Node {
        let mut children = Vec::new();
        children.push(Element::Node(self.parse_match_pattern()));
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::FatArrow)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        Node::new(NodeKind::MatchArm, children)
    }

    fn parse_match_pattern(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        match self.current_non_trivia_kind() {
            Some(TokenKind::Ident) => {
                children.push(Element::Node(self.parse_type_path()));
                self.collect_trivia(&mut children);
                if self.at(TokenKind::LParen) {
                    children.push(Element::Token(self.bump()));
                    self.collect_trivia(&mut children);
                    children.push(Element::Token(self.expect(TokenKind::Ident)));
                    self.collect_trivia(&mut children);
                    children.push(Element::Token(self.expect(TokenKind::RParen)));
                }
            }
            Some(TokenKind::Integer) => {
                children.push(Element::Token(self.expect(TokenKind::Integer)));
            }
            Some(TokenKind::String) => {
                children.push(Element::Token(self.expect(TokenKind::String)));
            }
            Some(TokenKind::KwTrue) => {
                children.push(Element::Token(self.expect(TokenKind::KwTrue)));
            }
            Some(TokenKind::KwFalse) => {
                children.push(Element::Token(self.expect(TokenKind::KwFalse)));
            }
            Some(TokenKind::Underscore) => {
                children.push(Element::Token(self.expect(TokenKind::Underscore)));
            }
            _ => {
                children.push(Element::Token(self.bump()));
            }
        }
        Node::new(NodeKind::MatchPattern, children)
    }

    fn parse_repeat_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::KwRepeat)));
        self.collect_trivia(&mut children);
        let repeat_has_binding = if self.at(TokenKind::Ident) {
            let mut index = self.cursor + 1;
            while self
                .tokens
                .get(index)
                .is_some_and(|token| token.kind.is_trivia())
            {
                index += 1;
            }
            self.tokens
                .get(index)
                .is_some_and(|token| token.kind == TokenKind::KwIn)
        } else {
            false
        };
        if repeat_has_binding {
            children.push(Element::Token(self.bump()));
            self.collect_trivia(&mut children);
            children.push(Element::Token(self.expect(TokenKind::KwIn)));
            self.collect_trivia(&mut children);
        }
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        Node::new(NodeKind::ExprRepeat, children)
    }

    fn parse_while_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::KwWhile)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_bp(0)));
        self.collect_trivia(&mut children);
        children.push(Element::Node(self.parse_expr_body()));
        Node::new(NodeKind::ExprWhile, children)
    }

    fn parse_array_expr(&mut self) -> Node {
        let mut children = Vec::new();
        self.collect_trivia(&mut children);
        children.push(Element::Token(self.expect(TokenKind::LBracket)));

        while !self.at(TokenKind::RBracket) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RBracket) || self.at(TokenKind::Eof) {
                break;
            }

            children.push(Element::Node(self.parse_expr_bp(0)));
            self.collect_trivia(&mut children);

            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        children.push(Element::Token(self.expect(TokenKind::RBracket)));
        Node::new(NodeKind::ExprArray, children)
    }

    fn parse_arg_list(&mut self) -> Node {
        let mut children = Vec::new();
        let offset = self.current().span.start;

        while !self.at(TokenKind::RParen) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RParen) || self.at(TokenKind::Eof) {
                break;
            }

            children.push(Element::Node(self.parse_expr_bp(0)));
            self.collect_trivia(&mut children);

            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if children.is_empty() {
            Node::empty(NodeKind::ArgList, offset)
        } else {
            Node::new(NodeKind::ArgList, children)
        }
    }

    fn parse_field_init_list(&mut self) -> Node {
        let mut children = Vec::new();
        let offset = self.current().span.start;

        while !self.at(TokenKind::RBrace) && !self.at(TokenKind::Eof) {
            self.collect_trivia(&mut children);

            if self.at(TokenKind::RBrace) || self.at(TokenKind::Eof) {
                break;
            }

            let mut init_children = Vec::new();
            self.collect_trivia(&mut init_children);
            init_children.push(Element::Token(self.expect(TokenKind::Ident)));
            self.collect_trivia(&mut init_children);
            init_children.push(Element::Token(self.expect(TokenKind::Colon)));
            self.collect_trivia(&mut init_children);
            init_children.push(Element::Node(self.parse_expr_bp(0)));
            children.push(Element::Node(Node::new(NodeKind::FieldInit, init_children)));
            self.collect_trivia(&mut children);

            if self.at(TokenKind::Comma) {
                children.push(Element::Token(self.bump()));
            } else {
                break;
            }
        }

        if children.is_empty() {
            Node::empty(NodeKind::FieldInitList, offset)
        } else {
            Node::new(NodeKind::FieldInitList, children)
        }
    }

    fn parse_error_expr(&mut self) -> Node {
        let token = self.bump();
        self.diagnostics.push(Diagnostic::new(
            "parse.expected-expression",
            format!("expected expression, found `{:?}`", token.kind),
            token.span,
                    Some(
                        "Use a literal, array literal, name, record literal, field access, indexing expression, grouped expression, function call, `if`, `match`, or `repeat`."
                            .to_owned(),
                    ),
                ));
        Node::new(NodeKind::Error, vec![Element::Token(token)])
    }

    fn collect_trivia(&mut self, children: &mut Vec<Element>) {
        while self.current().kind.is_trivia() {
            children.push(Element::Token(self.bump()));
        }
    }

    fn starts_match_pattern(&self) -> bool {
        matches!(
            self.current_non_trivia_kind(),
            Some(
                TokenKind::Ident
                    | TokenKind::Integer
                    | TokenKind::String
                    | TokenKind::KwTrue
                    | TokenKind::KwFalse
                    | TokenKind::Underscore
            )
        )
    }

    fn expect(&mut self, kind: TokenKind) -> Token {
        self.collect_trivia_into_cursor();

        if self.at(kind) {
            self.bump()
        } else {
            let current = self.current().clone();
            self.diagnostics.push(Diagnostic::new(
                "parse.expected-token",
                format!("expected `{:?}`, found `{:?}`", kind, current.kind),
                current.span,
                Some(format!(
                    "Insert or replace the token so this item matches the grammar for `{kind:?}`."
                )),
            ));
            Token::new(kind, String::new(), current.span)
        }
    }

    fn collect_trivia_into_cursor(&mut self) {
        while self.current().kind.is_trivia() {
            self.cursor += 1;
        }
    }

    fn next_non_trivia_kind_is_numeric_literal(&self) -> bool {
        let mut index = self.cursor + 1;
        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }
        matches!(
            self.tokens.get(index).map(|token| token.kind),
            Some(TokenKind::Integer | TokenKind::Float)
        )
    }

    fn peek_trivia_then(&self, kind: TokenKind) -> bool {
        let mut index = self.cursor;

        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }

        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == kind)
    }

    fn peek_non_trivia_kind(&self) -> Option<TokenKind> {
        let mut index = self.cursor;

        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }

        self.tokens.get(index).map(|token| token.kind)
    }

    fn current_non_trivia_kind(&self) -> Option<TokenKind> {
        self.peek_non_trivia_kind()
    }

    fn starts_record_literal(&self) -> bool {
        let mut index = self.cursor;
        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }
        if !self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::LBrace)
        {
            return false;
        }
        index += 1;
        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }
        if !self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Ident)
        {
            return false;
        }
        index += 1;
        while self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind.is_trivia())
        {
            index += 1;
        }
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Colon)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn current(&self) -> &Token {
        self.tokens
            .get(self.cursor)
            .unwrap_or_else(|| self.tokens.last().expect("parser requires EOF token"))
    }

    fn bump(&mut self) -> Token {
        let token = self.current().clone();
        if self.cursor < self.tokens.len().saturating_sub(1) {
            self.cursor += 1;
        }
        token
    }
}

const fn infix_binding_power(kind: TokenKind) -> Option<(u8, u8)> {
    match kind {
        TokenKind::KwOr => Some((1, 2)),
        TokenKind::KwAnd => Some((3, 4)),
        TokenKind::EqEq
        | TokenKind::NotEq
        | TokenKind::Lt
        | TokenKind::Le
        | TokenKind::Gt
        | TokenKind::Ge => Some((5, 6)),
        TokenKind::Plus | TokenKind::Minus => Some((7, 8)),
        TokenKind::Star | TokenKind::Slash => Some((9, 10)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::lexer::lex;
    use crate::{Element, NodeKind, TokenKind};

    use crate::parser::parse;

    #[test]
    fn parses_a_function_item() {
        let lexed = lex(
            "fn main() -> I32 effects [io] requires true ensures result == 3 { add(1, 2) + 3 }",
        );
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.root.kind, NodeKind::SourceFile);
        assert!(parsed.root.children.iter().any(|child| matches!(
            child,
            Element::Node(node) if node.kind == NodeKind::FnItem
        )));
    }

    #[test]
    fn parses_not_as_a_prefix_expression() {
        let lexed = lex("fn main() -> Bool { not false and true }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.root.children.iter().any(|child| matches!(
            child,
            Element::Node(node) if node.kind == NodeKind::FnItem
        )));
    }

    #[test]
    fn parses_const_items() {
        let lexed = lex("const answer: I32 = 40 + 2;\nfn main() -> I32 { answer }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.root.children.iter().any(|child| matches!(
            child,
            Element::Node(node) if node.kind == NodeKind::ConstItem
        )));
    }

    #[test]
    fn parses_array_types_in_declarations() {
        let lexed =
            lex("struct Grid { rows: [[I32; 2]; 2], }\nfn sum(xs: [I32; 2]) -> [I32; 2] { xs }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        let tree = parsed.root.pretty();
        assert!(tree.contains("TypeArray"));
    }

    #[test]
    fn parses_enum_and_match_expression() {
        let lexed = lex(
            "enum Color { red, green }\nfn main() -> I32 { match Color.red { Color.red => { 1 }, Color.green => { 2 }, } }",
        );
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        assert!(
            parsed.root.children.iter().any(
                |child| matches!(child, Element::Node(node) if node.kind == NodeKind::EnumItem)
            )
        );
        let tree = parsed.root.pretty();
        assert!(tree.contains("ExprMatch"));
        assert!(tree.contains("MatchArm"));
    }

    #[test]
    fn parses_scalar_and_wildcard_match_patterns() {
        let lexed = lex("fn main() -> I32 { match 41 { 40 => { 0 }, 41 => { 1 }, _ => { 2 }, } }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        let tree = parsed.root.pretty();
        assert!(tree.contains("ExprMatch"));
        assert!(tree.contains("MatchPattern"));
    }

    #[test]
    fn parses_float_literals_in_expressions() {
        let lexed = lex("fn main() -> F64 { 3.5 }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        let tree = parsed.root.pretty();
        assert!(tree.contains("ExprFloat"));
        assert!(tree.contains("Float"));
    }

    #[test]
    fn parses_expression_precedence_inside_function_bodies() {
        let lexed = lex("fn main() -> I32 requires 1 + 2 * 3 == 7 and true { 1 + 2 * 3 }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert!(body.pretty().contains("ExprBinary"));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_record_literals_and_field_access() {
        let lexed = lex(
            "struct Pair { left: I32, right: Bool }\nfn main() -> Bool { Pair { left: 7, right: true }.right }",
        );
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        let tree = parsed.root.pretty();
        assert!(tree.contains("ExprRecord"));
        assert!(tree.contains("FieldInitList"));
        assert!(tree.contains("ExprField"));
    }

    #[test]
    fn parses_array_literals_and_indexing() {
        let lexed = lex("fn main() -> I32 { let xs = [20, 22]; xs[0] + xs[1] }");
        let parsed = parse(&lexed.tokens);

        assert!(parsed.diagnostics.is_empty());
        let tree = parsed.root.pretty();
        assert!(tree.contains("ExprArray"));
        assert!(tree.contains("ExprIndex"));
    }

    #[test]
    fn parses_let_bindings_inside_function_bodies() {
        let lexed = lex("fn main() -> I32 { let x = 1; let y = 2; x + y }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert_eq!(
            body.children
                .iter()
                .filter(
                    |child| matches!(child, Element::Node(node) if node.kind == NodeKind::LetStmt)
                )
                .count(),
            2
        );
        assert!(body.pretty().contains("ExprBinary"));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_if_expression_with_branch_bodies() {
        let lexed = lex("fn main() -> I32 { if true { let x = 1; x } else { 2 } }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert!(
            body.children
                .iter()
                .any(|child| matches!(child, Element::Node(node) if node.kind == NodeKind::ExprIf))
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_repeat_expression_with_body() {
        let lexed = lex("fn main() { repeat 3 { let value = 1; value } }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert!(body.children.iter().any(
            |child| matches!(child, Element::Node(node) if node.kind == NodeKind::ExprRepeat)
        ));
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_indexed_repeat_expression_with_body() {
        let lexed = lex("fn main() { repeat i in 3 { i } }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        let repeat = body
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::ExprRepeat => Some(node),
                _ => None,
            })
            .expect("repeat expression");

        assert!(
            repeat.children.iter().any(
                |child| matches!(child, Element::Token(token) if token.kind == TokenKind::KwIn)
            )
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_while_expression_with_body() {
        let lexed = lex("fn main() { while true { let value = 1; value; } }");
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert!(
            body.children.iter().any(
                |child| matches!(child, Element::Node(node) if node.kind == NodeKind::ExprWhile)
            )
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_unit_expression_statements_inside_function_bodies() {
        let lexed = lex(
            "fn main() -> I32 { repeat 2 { 0 }; if true { repeat 1 { 0 } } else { repeat 1 { 0 } }; 42 }",
        );
        let parsed = parse(&lexed.tokens);
        let function = parsed
            .root
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .expect("function item");
        let body = function
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert_eq!(
            body.children
                .iter()
                .filter(
                    |child| matches!(child, Element::Node(node) if node.kind == NodeKind::ExprStmt)
                )
                .count(),
            2
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn parses_call_expression_statements_inside_function_bodies() {
        let lexed = lex("fn ping() {}\nfn main() -> I32 { ping(); 42 }");
        let parsed = parse(&lexed.tokens);
        let main = parsed
            .root
            .children
            .iter()
            .filter_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::FnItem => Some(node),
                _ => None,
            })
            .find(|node| {
                node.children.iter().any(|child| {
                    matches!(
                        child,
                        Element::Token(token)
                            if token.kind == TokenKind::Ident && token.lexeme == "main"
                    )
                })
            })
            .expect("main function");
        let body = main
            .children
            .iter()
            .find_map(|child| match child {
                Element::Node(node) if node.kind == NodeKind::Body => Some(node),
                _ => None,
            })
            .expect("body");

        assert_eq!(
            body.children
                .iter()
                .filter(
                    |child| matches!(child, Element::Node(node) if node.kind == NodeKind::ExprStmt)
                )
                .count(),
            1
        );
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn reports_an_unclosed_block() {
        let lexed = lex("fn main() {");
        let parsed = parse(&lexed.tokens);

        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].code, "parse.expected-token");
        assert_eq!(lexed.tokens.last().expect("EOF").kind, TokenKind::Eof);
    }
}
