/// The structure of a document.
struct AST {
    metadata: Metadata,
    main_section: MainSection,
    extra_sections: ExtraSection,
}

/// This is part of a document that behaves like a word processor.
struct MainSection {
    elements: Vec<Element>,
}

/// An element can be a paragraph, a heading, a table, etc.
struct Element {
    id: ElementId,
    kind: ElementKind,
    wrapper: Option<ElementWrapper>,
}

struct ElementId();

enum ElementKind {
    Paragraph,
    Heading,
    Table,
    Image,
    Equation,
    Quote,
}

struct ElementWrapper {}

/// An extra document section is a dynamic form.
struct ExtraSection {}

struct Metadata {
    name: String,
    settings: Vec<Setting>,
    template: Template,
}

struct Setting {}

struct Template {
    id: String,
    name: String,
    variant: TemplateVariant,
}

struct TemplateVariant {}
