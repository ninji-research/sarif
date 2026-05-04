# Sarif Semantic Docs


## bootstrap/sarif_syntax/src/main.sarif

### struct Span

- ownership: `plain value`
- rt status: `profile-compatible`

### struct OptionalSpan

- ownership: `plain value`
- rt status: `profile-compatible`

### enum TopLevelKind

- variants: `4`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct TopLevelEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct OptionalTopLevelEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct TopLevelOutline

- ownership: `plain value`
- rt status: `profile-compatible`

### struct FnOutlineEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct OptionalFnOutlineEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct FnOutline

- ownership: `plain value`
- rt status: `profile-compatible`

### struct FnHeaderShape

- ownership: `plain value`
- rt status: `profile-compatible`

### enum BlockItemKind

- variants: `4`
- ownership: `plain tag`
- rt status: `profile-compatible`

### enum ExprKind

- variants: `15`
- ownership: `plain tag`
- rt status: `profile-compatible`

### enum MirInst

- variants: `53`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct MirInstData

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct ValueId

- ownership: `plain value`
- rt status: `profile-compatible`

### struct LocalSlotId

- ownership: `plain value`
- rt status: `profile-compatible`

### enum MirType

- variants: `6`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct MirParam

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirLocal

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirFn

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirType

- ownership: `plain value`
- rt status: `profile-compatible`

### struct MirParamList

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirParam

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirFxList

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirFx

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirLocalList

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirLocal

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirBlock

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirInst

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirProg

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct MirFnList

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct OptionalMirFn

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct BlockItemEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct OptionalBlockItemEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct BlockOutline

- ownership: `plain value`
- rt status: `profile-compatible`

### enum SyntaxEventKind

- variants: `13`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct SyntaxEvent

- ownership: `plain value`
- rt status: `profile-compatible`

### struct OptionalSyntaxEvent

- ownership: `plain value`
- rt status: `profile-compatible`

### struct EventStream

- ownership: `plain value`
- rt status: `profile-compatible`

### enum TokenKind

- variants: `57`
- ownership: `plain tag`
- rt status: `profile-compatible`

### enum ByteClass

- variants: `4`
- ownership: `plain tag`
- rt status: `profile-compatible`

### enum LeadClass

- variants: `5`
- ownership: `plain tag`
- rt status: `profile-compatible`

### enum ListKind

- variants: `6`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct Token

- ownership: `plain value`
- rt status: `profile-compatible`

### enum ParseStatus

- variants: `2`
- ownership: `plain tag`
- rt status: `profile-compatible`

### struct TypeSection

- ownership: `plain value`
- rt status: `profile-compatible`

### struct ItemSection

- ownership: `plain value`
- rt status: `profile-compatible`

### struct TopLevelReport

- ownership: `plain value`
- rt status: `profile-compatible`

### struct ParseState

- ownership: `plain value`
- rt status: `profile-compatible`

### struct ModuleReport

- ownership: `contains affine fields`
- rt status: `blocked in rt`

### struct BlockEntry

- ownership: `plain value`
- rt status: `profile-compatible`

### struct TypeSectionParse

- ownership: `plain value`
- rt status: `profile-compatible`

### struct ItemSectionParse

- ownership: `plain value`
- rt status: `profile-compatible`

### struct FnItemParse

- ownership: `plain value`
- rt status: `profile-compatible`

### struct SpannedParse

- ownership: `plain value`
- rt status: `profile-compatible`

### fn make_token

- signature: `fn make_token(kind: TokenKind, start: I32, end: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_span

- signature: `fn make_span(start: I32, end: I32) -> Span`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_optional_span

- signature: `fn make_optional_span(present: Bool, span: Span) -> OptionalSpan`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_fn_header_shape

- signature: `fn make_fn_header_shape(params_span: OptionalSpan, return_span: OptionalSpan, effects_span: OptionalSpan, requires_span: OptionalSpan, ensures_span: OptionalSpan, body_span: OptionalSpan, body_outline: BlockOutline) -> FnHeaderShape`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_block_item_entry

- signature: `fn make_block_item_entry(kind: BlockItemKind, binding_span: OptionalSpan, expr_span: OptionalSpan, expr_kind: ExprKind) -> BlockItemEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_syntax_event

- signature: `fn make_syntax_event(kind: SyntaxEventKind, span: OptionalSpan, name_span: OptionalSpan, expr_kind: ExprKind) -> SyntaxEvent`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_optional_syntax_event

- signature: `fn make_optional_syntax_event(present: Bool, event: SyntaxEvent) -> OptionalSyntaxEvent`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_optional_block_item_entry

- signature: `fn make_optional_block_item_entry(present: Bool, entry: BlockItemEntry) -> OptionalBlockItemEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_event_stream

- signature: `fn make_event_stream(total_count: I32, truncated: Bool, first: OptionalSyntaxEvent, second: OptionalSyntaxEvent, third: OptionalSyntaxEvent, fourth: OptionalSyntaxEvent, fifth: OptionalSyntaxEvent, sixth: OptionalSyntaxEvent, seventh: OptionalSyntaxEvent, eighth: OptionalSyntaxEvent, ninth: OptionalSyntaxEvent, tenth: OptionalSyntaxEvent, eleventh: OptionalSyntaxEvent, twelfth: OptionalSyntaxEvent, thirteenth: OptionalSyntaxEvent, fourteenth: OptionalSyntaxEvent, fifteenth: OptionalSyntaxEvent, sixteenth: OptionalSyntaxEvent) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_block_outline

- signature: `fn make_block_outline(total_count: I32, truncated: Bool, first: OptionalBlockItemEntry, second: OptionalBlockItemEntry, third: OptionalBlockItemEntry, fourth: OptionalBlockItemEntry, fifth: OptionalBlockItemEntry) -> BlockOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_block_item_entry

- signature: `fn missing_block_item_entry() -> OptionalBlockItemEntry`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn block_outline_new

- signature: `fn block_outline_new() -> BlockOutline`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn block_outline_push

- signature: `fn block_outline_push(outline: BlockOutline, entry: OptionalBlockItemEntry) -> BlockOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_syntax_event

- signature: `fn missing_syntax_event() -> OptionalSyntaxEvent`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn event_stream_new

- signature: `fn event_stream_new() -> EventStream`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn event_stream_push

- signature: `fn event_stream_push(stream: EventStream, event: OptionalSyntaxEvent) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_fn_header_shape

- signature: `fn missing_fn_header_shape() -> FnHeaderShape`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn make_top_level_entry

- signature: `fn make_top_level_entry(kind: TopLevelKind, span: Span, name_span: OptionalSpan) -> TopLevelEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_optional_top_level_entry

- signature: `fn make_optional_top_level_entry(present: Bool, entry: TopLevelEntry) -> OptionalTopLevelEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_span

- signature: `fn missing_span() -> OptionalSpan`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn present_span

- signature: `fn present_span(start: I32, end: I32) -> OptionalSpan`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_top_level_entry

- signature: `fn missing_top_level_entry() -> OptionalTopLevelEntry`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn present_top_level_entry

- signature: `fn present_top_level_entry(kind: TopLevelKind, start: I32, end: I32, name_span: OptionalSpan) -> OptionalTopLevelEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn span_if_items

- signature: `fn span_if_items(count: I32, start: I32, end: I32) -> OptionalSpan`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_parse_state

- signature: `fn make_parse_state(cursor: Token, status: ParseStatus, last_end: I32) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_type_section

- signature: `fn make_type_section(struct_count: I32, enum_count: I32, span: OptionalSpan) -> TypeSection`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_item_section

- signature: `fn make_item_section(const_count: I32, fn_count: I32, const_span: OptionalSpan, fn_span: OptionalSpan) -> ItemSection`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_top_level_report

- signature: `fn make_top_level_report(types: TypeSection, items: ItemSection) -> TopLevelReport`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_fn_outline_entry

- signature: `fn make_fn_outline_entry(shape: FnHeaderShape) -> FnOutlineEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_optional_fn_outline_entry

- signature: `fn make_optional_fn_outline_entry(present: Bool, entry: FnOutlineEntry) -> OptionalFnOutlineEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn missing_fn_outline_entry

- signature: `fn missing_fn_outline_entry() -> OptionalFnOutlineEntry`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn present_fn_outline_entry

- signature: `fn present_fn_outline_entry(shape: FnHeaderShape) -> OptionalFnOutlineEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_fn_outline

- signature: `fn make_fn_outline(total_count: I32, truncated: Bool, first: OptionalFnOutlineEntry, second: OptionalFnOutlineEntry, third: OptionalFnOutlineEntry, fourth: OptionalFnOutlineEntry, fifth: OptionalFnOutlineEntry) -> FnOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn fn_outline_new

- signature: `fn fn_outline_new() -> FnOutline`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn fn_outline_push

- signature: `fn fn_outline_push(outline: FnOutline, entry: OptionalFnOutlineEntry) -> FnOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_module_report

- signature: `fn make_module_report(ok: Bool, top_level: TopLevelReport, outline: TopLevelOutline, fn_outline: FnOutline, events: EventStream, module_span: Span, diagnostics: Text) -> ModuleReport`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn make_top_level_outline

- signature: `fn make_top_level_outline(total_count: I32, truncated: Bool, first: OptionalTopLevelEntry, second: OptionalTopLevelEntry, third: OptionalTopLevelEntry, fourth: OptionalTopLevelEntry, fifth: OptionalTopLevelEntry) -> TopLevelOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn top_level_outline_new

- signature: `fn top_level_outline_new() -> TopLevelOutline`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn top_level_outline_push

- signature: `fn top_level_outline_push(outline: TopLevelOutline, entry: OptionalTopLevelEntry) -> TopLevelOutline`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn top_level_entry_at

- signature: `fn top_level_entry_at(outline: TopLevelOutline, index: I32) -> OptionalTopLevelEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn fn_outline_entry_at

- signature: `fn fn_outline_entry_at(outline: FnOutline, index: I32) -> OptionalFnOutlineEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn block_item_entry_at

- signature: `fn block_item_entry_at(outline: BlockOutline, index: I32) -> OptionalBlockItemEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn syntax_event_at

- signature: `fn syntax_event_at(stream: EventStream, index: I32) -> OptionalSyntaxEvent`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn syntax_event_kind_matches

- signature: `fn syntax_event_kind_matches(stream: EventStream, index: I32, kind: SyntaxEventKind) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn syntax_event_expr_matches

- signature: `fn syntax_event_expr_matches(stream: EventStream, index: I32, kind: SyntaxEventKind, expr_kind: ExprKind) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn top_level_event_kind

- signature: `fn top_level_event_kind(kind: TopLevelKind) -> SyntaxEventKind`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn block_event_kind

- signature: `fn block_event_kind(kind: BlockItemKind) -> SyntaxEventKind`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn append_clause_event

- signature: `fn append_clause_event(stream: EventStream, kind: SyntaxEventKind, span: OptionalSpan) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn append_block_events

- signature: `fn append_block_events(stream: EventStream, outline: BlockOutline) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn append_fn_shape_events

- signature: `fn append_fn_shape_events(stream: EventStream, shape: FnHeaderShape) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn build_event_stream

- signature: `fn build_event_stream(outline: TopLevelOutline, fn_outline: FnOutline) -> EventStream`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn module_report_from_state

- signature: `fn module_report_from_state(state: ParseState, top_level: TopLevelReport, outline: TopLevelOutline, fn_outline: FnOutline, source: Text) -> ModuleReport`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn make_block_entry

- signature: `fn make_block_entry(state: ParseState, tail_seen: Bool) -> BlockEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_block_entry_with_item

- signature: `fn make_block_entry_with_item(state: ParseState, tail_seen: Bool, item: OptionalBlockItemEntry) -> BlockEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_type_section_parse

- signature: `fn make_type_section_parse(state: ParseState, section: TypeSection, outline: TopLevelOutline) -> TypeSectionParse`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_item_section_parse

- signature: `fn make_item_section_parse(state: ParseState, section: ItemSection, outline: TopLevelOutline, fn_outline: FnOutline) -> ItemSectionParse`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_fn_item_parse

- signature: `fn make_fn_item_parse(state: ParseState, shape: FnHeaderShape) -> FnItemParse`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_spanned_parse

- signature: `fn make_spanned_parse(state: ParseState, span: OptionalSpan) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn present_block_item_entry

- signature: `fn present_block_item_entry(kind: BlockItemKind, binding_span: OptionalSpan, expr_span: OptionalSpan, expr_kind: ExprKind) -> OptionalBlockItemEntry`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn report_matches_sample

- signature: `fn report_matches_sample(report: ModuleReport) -> Bool`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn report_matches_empty_module

- signature: `fn report_matches_empty_module(report: ModuleReport) -> Bool`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn report_score

- signature: `fn report_score(report: ModuleReport) -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn i32_from_bool

- signature: `fn i32_from_bool(value: Bool) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn top_level_name_span

- signature: `fn top_level_name_span(source: Text, state: ParseState) -> OptionalSpan`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn is_whitespace

- signature: `fn is_whitespace(b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn is_newline

- signature: `fn is_newline(b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn is_alpha

- signature: `fn is_alpha(b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn is_digit

- signature: `fn is_digit(b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn is_trivia

- signature: `fn is_trivia(kind: TokenKind) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn is_ident_continue

- signature: `fn is_ident_continue(b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn matches_byte_class

- signature: `fn matches_byte_class(class: ByteClass, b: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn classify_lead_byte

- signature: `fn classify_lead_byte(b: I32) -> LeadClass`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn scan_while

- signature: `fn scan_while(source: Text, offset: I32, len: I32, class: ByteClass) -> I32`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn scan_comment

- signature: `fn scan_comment(source: Text, offset: I32, len: I32) -> I32`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn scan_string

- signature: `fn scan_string(source: Text, offset: I32, len: I32) -> I32`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn text_eq_range

- signature: `fn text_eq_range(source: Text, start: I32, end: I32, expected: Text) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn make_single

- signature: `fn make_single(kind: TokenKind, offset: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn make_double

- signature: `fn make_double(kind: TokenKind, offset: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn next_is

- signature: `fn next_is(source: Text, offset: I32, len: I32, expected: I32) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn keyword_or_ident

- signature: `fn keyword_or_ident(source: Text, start: I32, end: I32, expected: Text, kind: TokenKind) -> TokenKind`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn classify_ident

- signature: `fn classify_ident(source: Text, start: I32, end: I32) -> TokenKind`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn lex_symbol

- signature: `fn lex_symbol(source: Text, offset: I32, len: I32, b: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn lex_next

- signature: `fn lex_next(source: Text, offset: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn raw_cursor_new

- signature: `fn raw_cursor_new(source: Text) -> Token`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn raw_cursor_bump

- signature: `fn raw_cursor_bump(source: Text, cursor: Token) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn skip_trivia

- signature: `fn skip_trivia(source: Text, cursor: Token) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn next_significant

- signature: `fn next_significant(source: Text, offset: I32) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn cursor_new

- signature: `fn cursor_new(source: Text) -> Token`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn cursor_bump

- signature: `fn cursor_bump(source: Text, cursor: Token) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn cursor_at

- signature: `fn cursor_at(cursor: Token, kind: TokenKind) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn parse_state_new

- signature: `fn parse_state_new(source: Text) -> ParseState`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn with_parse_state

- signature: `fn with_parse_state(state: ParseState, cursor: Token, status: ParseStatus) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn with_last_end

- signature: `fn with_last_end(state: ParseState, last_end: I32) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn parse_advance

- signature: `fn parse_advance(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_expect

- signature: `fn parse_expect(source: Text, state: ParseState, kind: TokenKind) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_finished

- signature: `fn parse_finished(state: ParseState) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn parse_fail

- signature: `fn parse_fail(state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn infix_left_bp

- signature: `fn infix_left_bp(kind: TokenKind) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn infix_right_bp

- signature: `fn infix_right_bp(kind: TokenKind) -> I32`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn starts_expr

- signature: `fn starts_expr(kind: TokenKind) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn next_after_name

- signature: `fn next_after_name(source: Text, cursor: Token) -> Token`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn starts_assign

- signature: `fn starts_assign(source: Text, state: ParseState) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn starts_record_literal

- signature: `fn starts_record_literal(source: Text, state: ParseState) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_list_item

- signature: `fn parse_list_item(source: Text, state: ParseState, kind: ListKind) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_comma_list

- signature: `fn parse_comma_list(source: Text, state: ParseState, end_kind: TokenKind, kind: ListKind) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_field_init

- signature: `fn parse_field_init(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_field_init_list

- signature: `fn parse_field_init_list(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_arg_list

- signature: `fn parse_arg_list(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_array_expr

- signature: `fn parse_array_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_if_expr

- signature: `fn parse_if_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_payload_pattern

- signature: `fn parse_payload_pattern(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_match_pattern

- signature: `fn parse_match_pattern(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_match_expr

- signature: `fn parse_match_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_repeat_expr

- signature: `fn parse_repeat_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_while_expr

- signature: `fn parse_while_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_postfix_expr

- signature: `fn parse_postfix_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_prefix_expr

- signature: `fn parse_prefix_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_ident_expr

- signature: `fn parse_ident_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_primary_expr

- signature: `fn parse_primary_expr(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_expr_bp

- signature: `fn parse_expr_bp(source: Text, state: ParseState, min_bp: I32) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_contract_clause

- signature: `fn parse_contract_clause(source: Text, state: ParseState, keyword: TokenKind) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_const_item

- signature: `fn parse_const_item(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_named_type

- signature: `fn parse_named_type(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_array_type

- signature: `fn parse_array_type(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_type

- signature: `fn parse_type(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_param

- signature: `fn parse_param(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_ident_span

- signature: `fn parse_ident_span(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_param_list

- signature: `fn parse_param_list(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_fn_params

- signature: `fn parse_fn_params(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_field

- signature: `fn parse_field(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_field_list

- signature: `fn parse_field_list(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_variant

- signature: `fn parse_variant(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_variant_list

- signature: `fn parse_variant_list(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_struct_item

- signature: `fn parse_struct_item(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_enum_item

- signature: `fn parse_enum_item(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_optional_return_type

- signature: `fn parse_optional_return_type(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_effects_clause

- signature: `fn parse_effects_clause(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_optional_effects_clause

- signature: `fn parse_optional_effects_clause(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_optional_requires_clause

- signature: `fn parse_optional_requires_clause(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_optional_ensures_clause

- signature: `fn parse_optional_ensures_clause(source: Text, state: ParseState) -> SpannedParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_stmt

- signature: `fn parse_stmt(source: Text, state: ParseState) -> BlockEntry`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn classify_expr_kind

- signature: `fn classify_expr_kind(source: Text, state: ParseState) -> ExprKind`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_block_with_outline

- signature: `fn parse_block_with_outline(source: Text, state: ParseState) -> FnItemParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_block

- signature: `fn parse_block(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_fn_item

- signature: `fn parse_fn_item(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_fn_item_with_shape

- signature: `fn parse_fn_item_with_shape(source: Text, state: ParseState) -> FnItemParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_item

- signature: `fn parse_item(source: Text, state: ParseState) -> ParseState`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_type_section_report

- signature: `fn parse_type_section_report(source: Text, state: ParseState) -> TypeSectionParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_item_section_report

- signature: `fn parse_item_section_report(source: Text, state: ParseState, outline_seed: TopLevelOutline) -> ItemSectionParse`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn parse_module_report

- signature: `fn parse_module_report(source: Text) -> ModuleReport`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn syntax_selfcheck

- signature: `fn syntax_selfcheck() -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn parse_f64

- signature: `fn parse_f64(source: Text) -> F64`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn text_from_f64_fixed

- signature: `fn text_from_f64_fixed(value: F64, digits: I32) -> Text`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn mir_prog_new

- signature: `fn mir_prog_new() -> MirProg`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_fn_new

- signature: `fn mir_fn_new(name: Text) -> MirFn`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn mir_inst_data_new

- signature: `fn mir_inst_data_new(tag: MirInst) -> MirInstData`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn value_id_0

- signature: `fn value_id_0() -> ValueId`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn value_id_new

- signature: `fn value_id_new(i: I32) -> ValueId`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn slot_id_0

- signature: `fn slot_id_0() -> LocalSlotId`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn slot_id_new

- signature: `fn slot_id_new(i: I32) -> LocalSlotId`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn mir_type_i32

- signature: `fn mir_type_i32() -> MirType`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn mir_type_bool

- signature: `fn mir_type_bool() -> MirType`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn mir_type_unit

- signature: `fn mir_type_unit() -> MirType`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn mir_optional_type_false

- signature: `fn mir_optional_type_false() -> OptionalMirType`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

### fn mir_optional_param_false

- signature: `fn mir_optional_param_false() -> OptionalMirParam`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_optional_local_false

- signature: `fn mir_optional_local_false() -> OptionalMirLocal`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_optional_fx_false

- signature: `fn mir_optional_fx_false() -> OptionalMirFx`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_optional_fn_false

- signature: `fn mir_optional_fn_false() -> OptionalMirFn`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_optional_inst_false

- signature: `fn mir_optional_inst_false() -> OptionalMirInst`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_param_list_new

- signature: `fn mir_param_list_new() -> MirParamList`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_fx_list_new

- signature: `fn mir_fx_list_new() -> MirFxList`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_local_list_new

- signature: `fn mir_local_list_new() -> MirLocalList`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_block_new

- signature: `fn mir_block_new() -> MirBlock`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_fn_list_new

- signature: `fn mir_fn_list_new() -> MirFnList`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_inst_tag_is_alloc_push

- signature: `fn mir_inst_tag_is_alloc_push(tag: MirInst) -> Bool`
- ownership: `consumes affine arguments`
- rt status: `profile-compatible`

### fn mir_inst_tag_name

- signature: `fn mir_inst_tag_name(tag: MirInst) -> Text`
- ownership: `consumes affine arguments`
- rt status: `blocked in rt`

### fn mir_prog_check

- signature: `fn mir_prog_check(prog: MirProg) -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_fn_check

- signature: `fn mir_fn_check(func: MirFn) -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `blocked in rt`

### fn mir_selfcheck

- signature: `fn mir_selfcheck() -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`

## bootstrap/sarif_syntax/src/selfcheck.sarif

### fn main

- signature: `fn main() -> I32`
- ownership: `affine-safe in stage-0`
- rt status: `profile-compatible`


