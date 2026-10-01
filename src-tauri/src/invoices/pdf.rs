//! Invoice PDF composition. One US Letter document per invoice, drawn with
//! `printpdf` page operations and embedded Noto Sans (OFL, see
//! `assets/fonts/OFL.txt`). This is the only renderer: the editor previews these
//! exact bytes, "Export" saves them, and Finalize archives them.
//!
//! Coordinates are points from the top-left; `text` positions are baselines.
//! Glyph advances are measured from the font so right alignment and wrapping
//! are exact, but there is no kerning or complex-script shaping. Latin, Greek
//! and Cyrillic render; scripts outside Noto Sans (e.g. CJK) show as missing
//! glyphs.

use std::rc::Rc;

use printpdf::{
    Color, FontId, Line, LinePoint, Mm, Op, ParsedFont, PdfDocument, PdfPage, PdfSaveOptions,
    Point, Pt, RawImage, Rgb, TextItem, TextMatrix, XObjectId, XObjectTransform,
};

use super::money::{format_money, format_quantity};

const REGULAR_TTF: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");
const BOLD_TTF: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");

const PAGE_WIDTH: f32 = 612.0;
const PAGE_HEIGHT: f32 = 792.0;
const LEFT: f32 = 54.0;
const RIGHT: f32 = 558.0;
const CONTENT_BOTTOM: f32 = 716.0;
const FOOTER_RULE: f32 = 738.0;
const FOOTER_TEXT: f32 = 756.0;

const DESC_WIDTH: f32 = 290.0;
const QTY_RIGHT: f32 = 396.0;
const RATE_RIGHT: f32 = 476.0;
const AMOUNT_RIGHT: f32 = RIGHT;

const LOGO_MAX_WIDTH: f32 = 180.0;
const LOGO_MAX_HEIGHT: f32 = 56.0;

const LINE_HEIGHT: f32 = 13.5;
const BODY: f32 = 10.0;
const SMALL: f32 = 8.6;

type Rgbf = (f32, f32, f32);
const INK: Rgbf = (0.09, 0.10, 0.10);
const MUTED: Rgbf = (0.42, 0.43, 0.43);
const RULE: Rgbf = (0.83, 0.83, 0.81);
const WATERMARK: Rgbf = (0.93, 0.93, 0.92);

#[derive(Debug, Clone)]
pub struct PaperLine {
    pub description: String,
    /// A muted second line, e.g. `Project name · Sep 12, 2026`.
    pub detail: Option<String>,
    pub quantity_hundredths: i64,
    /// Short unit shown after the quantity, e.g. `hrs`; may be empty.
    pub unit: String,
    pub rate_cents: i64,
    pub amount_cents: i64,
}

/// Everything the paper shows, already validated and formatted where the
/// wording matters (dates), raw where the renderer formats (money, quantity).
#[derive(Debug, Clone)]
pub struct PaperInvoice {
    /// `None` renders a DRAFT: watermark and `Invoice: DRAFT`, no number.
    pub number: Option<String>,
    pub issue_date: String,
    pub due_date: String,
    pub terms: String,
    pub subject: Option<String>,
    pub issuer_name: String,
    pub issuer_lines: Vec<String>,
    pub logo: Option<Vec<u8>>,
    pub bill_to_name: String,
    pub bill_to_lines: Vec<String>,
    pub lines: Vec<PaperLine>,
    pub subtotal_cents: i64,
    /// Per-project you/agents lines, printed under the totals when agents
    /// contributed time; empty otherwise.
    pub splits: Vec<String>,
    pub payment_instructions: Option<String>,
    pub notes: Option<String>,
}

struct Fonts {
    regular: ParsedFont,
    bold: ParsedFont,
}

thread_local! {
    // `ParsedFont` is not `Sync`, so parse once per thread instead of once per app.
    static FONTS: std::cell::OnceCell<Option<Rc<Fonts>>> = const { std::cell::OnceCell::new() };
}

fn fonts() -> Result<Rc<Fonts>, String> {
    FONTS
        .with(|cell| {
            cell.get_or_init(|| {
                Some(Rc::new(Fonts {
                    regular: ParsedFont::from_bytes(REGULAR_TTF, 0, &mut Vec::new())?,
                    bold: ParsedFont::from_bytes(BOLD_TTF, 0, &mut Vec::new())?,
                }))
            })
            .clone()
        })
        .ok_or_else(|| "Could not load the bundled invoice font".to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum Weight {
    Regular,
    Bold,
}

fn color((r, g, b): Rgbf) -> Color {
    Color::Rgb(Rgb {
        r,
        g,
        b,
        icc_profile: None,
    })
}

fn point(x: f32, top: f32) -> Point {
    Point {
        x: Pt(x),
        y: Pt(PAGE_HEIGHT - top),
    }
}

/// Strips control characters and normalizes tabs so measuring and drawing agree.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\t' { ' ' } else { c })
        .filter(|c| *c == '\n' || !c.is_control())
        .collect()
}

struct Ctx {
    regular: FontId,
    bold: FontId,
    fonts: Rc<Fonts>,
}

impl Ctx {
    fn font(&self, weight: Weight) -> &ParsedFont {
        match weight {
            Weight::Regular => &self.fonts.regular,
            Weight::Bold => &self.fonts.bold,
        }
    }

    fn id(&self, weight: Weight) -> &FontId {
        match weight {
            Weight::Regular => &self.regular,
            Weight::Bold => &self.bold,
        }
    }

    fn width(&self, weight: Weight, text: &str, size: f32) -> f32 {
        let font = self.font(weight);
        let units = f32::from(font.font_metrics.units_per_em);
        // Outline-less glyphs (spaces) have no decoded record, so their advance
        // reads as zero; the font's space width stands in for any whitespace.
        let space = font.space_width.map_or(units / 4.0, |w| w as f32);
        text.chars()
            .map(|c| {
                let advance = if c.is_whitespace() {
                    space
                } else {
                    let glyph = font.lookup_glyph_index(u32::from(c)).unwrap_or(0);
                    f32::from(font.get_horizontal_advance(glyph))
                };
                advance * size / units
            })
            .sum()
    }

    /// Word-wraps to `max_width`, honoring newlines; a single word wider than
    /// the column is split between characters.
    fn wrap(&self, weight: Weight, text: &str, max_width: f32, size: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in clean(text).lines() {
            let mut current = String::new();
            for word in paragraph.split_whitespace() {
                let candidate = if current.is_empty() {
                    word.to_owned()
                } else {
                    format!("{current} {word}")
                };
                if self.width(weight, &candidate, size) <= max_width {
                    current = candidate;
                    continue;
                }
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                if self.width(weight, word, size) <= max_width {
                    current.push_str(word);
                    continue;
                }
                for c in word.chars() {
                    let mut next = current.clone();
                    next.push(c);
                    if !current.is_empty() && self.width(weight, &next, size) > max_width {
                        lines.push(std::mem::take(&mut current));
                    }
                    current.push(c);
                }
            }
            if !current.is_empty() {
                lines.push(current);
            }
            if paragraph.trim().is_empty() {
                lines.push(String::new());
            }
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        lines
    }
}

struct Page {
    ops: Vec<Op>,
}

impl Page {
    fn new() -> Self {
        Self { ops: Vec::new() }
    }

    fn text(&mut self, ctx: &Ctx, weight: Weight, value: &str, at: (f32, f32), size: f32, c: Rgbf) {
        let (x, top) = at;
        if value.is_empty() {
            return;
        }
        let font = ctx.id(weight).clone();
        self.ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor { pos: point(x, top) },
            Op::SetFontSize {
                size: Pt(size),
                font: font.clone(),
            },
            Op::SetFillColor { col: color(c) },
            Op::WriteText {
                items: vec![TextItem::Text(clean(value))],
                font,
            },
            Op::EndTextSection,
        ]);
    }

    fn text_right(
        &mut self,
        ctx: &Ctx,
        weight: Weight,
        value: &str,
        at: (f32, f32),
        size: f32,
        c: Rgbf,
    ) {
        let (right, top) = at;
        let x = right - ctx.width(weight, &clean(value), size);
        self.text(ctx, weight, value, (x, top), size, c);
    }

    fn rule(&mut self, x1: f32, x2: f32, top: f32) {
        self.ops.extend([
            Op::SetOutlineColor { col: color(RULE) },
            Op::SetOutlineThickness { pt: Pt(0.6) },
            Op::DrawLine {
                line: Line {
                    points: vec![
                        LinePoint {
                            p: point(x1, top),
                            bezier: false,
                        },
                        LinePoint {
                            p: point(x2, top),
                            bezier: false,
                        },
                    ],
                    is_closed: false,
                },
            },
        ]);
    }

    fn watermark(&mut self, ctx: &Ctx) {
        let size = 120.0;
        let width = ctx.width(Weight::Bold, "DRAFT", size);
        let angle = 45.0_f32;
        let (sin, cos) = angle.to_radians().sin_cos();
        // Text runs along the rotated baseline from its start point; center it.
        let x = PAGE_WIDTH / 2.0 - width / 2.0 * cos - size * 0.36 * -sin;
        let y = PAGE_HEIGHT / 2.0 - width / 2.0 * sin - size * 0.36 * cos;
        let font = ctx.bold.clone();
        self.ops.extend([
            Op::StartTextSection,
            Op::SetTextMatrix {
                matrix: TextMatrix::TranslateRotate(Pt(x), Pt(y), angle),
            },
            Op::SetFontSize {
                size: Pt(size),
                font: font.clone(),
            },
            Op::SetFillColor {
                col: color(WATERMARK),
            },
            Op::WriteText {
                items: vec![TextItem::Text("DRAFT".into())],
                font,
            },
            Op::EndTextSection,
        ]);
    }
}

enum RowText {
    Body(String),
    Detail(String),
}

/// Flows content down pages, adding a continuation page when it runs out.
struct Flow<'a> {
    ctx: &'a Ctx,
    draft: bool,
    number: String,
    pages: Vec<Page>,
    y: f32,
}

impl<'a> Flow<'a> {
    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a page always exists")
    }

    fn new_page(&mut self, with_table_header: bool) {
        let mut page = Page::new();
        if self.draft {
            page.watermark(self.ctx);
        }
        page.text(self.ctx, Weight::Bold, "Invoice", (LEFT, 68.0), 16.0, INK);
        page.text_right(
            self.ctx,
            Weight::Regular,
            &self.number,
            (RIGHT, 68.0),
            BODY,
            MUTED,
        );
        self.pages.push(page);
        self.y = 104.0;
        if with_table_header {
            self.table_header();
        }
    }

    fn table_header(&mut self) {
        let ctx = self.ctx;
        let y = self.y;
        let page = self.page();
        page.text(ctx, Weight::Regular, "DESCRIPTION", (LEFT, y), SMALL, MUTED);
        page.text_right(ctx, Weight::Regular, "QTY", (QTY_RIGHT, y), SMALL, MUTED);
        page.text_right(ctx, Weight::Regular, "RATE", (RATE_RIGHT, y), SMALL, MUTED);
        page.text_right(
            ctx,
            Weight::Regular,
            "AMOUNT",
            (AMOUNT_RIGHT, y),
            SMALL,
            MUTED,
        );
        page.rule(LEFT, RIGHT, y + 9.0);
        self.y = y + 27.0;
    }

    /// Starts a new page (repeating the table header) unless `height` fits.
    fn ensure(&mut self, height: f32, in_table: bool) {
        if self.y + height > CONTENT_BOTTOM {
            self.new_page(in_table);
        }
    }

    fn row(&mut self, line: &PaperLine) {
        let ctx = self.ctx;
        let mut lines: Vec<RowText> = ctx
            .wrap(Weight::Regular, &line.description, DESC_WIDTH, BODY)
            .into_iter()
            .map(RowText::Body)
            .collect();
        if let Some(detail) = &line.detail {
            lines.extend(
                ctx.wrap(Weight::Regular, detail, DESC_WIDTH, SMALL)
                    .into_iter()
                    .map(RowText::Detail),
            );
        }
        // Keep short rows whole; let very long ones flow line by line.
        let total = lines.len() as f32 * LINE_HEIGHT + 8.0;
        self.ensure(total.min(LINE_HEIGHT * 3.0 + 8.0), true);

        let quantity = if line.unit.is_empty() {
            format_quantity(line.quantity_hundredths)
        } else {
            format!(
                "{} {}",
                format_quantity(line.quantity_hundredths),
                line.unit
            )
        };
        let mut first = true;
        for text in &lines {
            self.ensure(LINE_HEIGHT + 4.0, true);
            let y = self.y;
            let page = self.page();
            match text {
                RowText::Body(body) => page.text(ctx, Weight::Regular, body, (LEFT, y), BODY, INK),
                RowText::Detail(detail) => {
                    page.text(ctx, Weight::Regular, detail, (LEFT, y), SMALL, MUTED)
                }
            }
            if first {
                page.text_right(ctx, Weight::Regular, &quantity, (QTY_RIGHT, y), 9.6, INK);
                page.text_right(
                    ctx,
                    Weight::Regular,
                    &format_money(line.rate_cents),
                    (RATE_RIGHT, y),
                    9.6,
                    INK,
                );
                page.text_right(
                    ctx,
                    Weight::Regular,
                    &format_money(line.amount_cents),
                    (AMOUNT_RIGHT, y),
                    9.6,
                    INK,
                );
                first = false;
            }
            self.y += LINE_HEIGHT;
        }
        self.y += 2.0;
        let y = self.y;
        self.page().rule(LEFT, RIGHT, y);
        self.y += 15.0;
    }

    fn totals(&mut self, subtotal_cents: i64) {
        self.ensure(70.0, false);
        let ctx = self.ctx;
        self.y += 6.0;
        let y = self.y;
        let page = self.page();
        page.text(ctx, Weight::Regular, "Subtotal", (380.0, y), BODY, MUTED);
        page.text_right(
            ctx,
            Weight::Bold,
            &format_money(subtotal_cents),
            (RIGHT, y),
            BODY,
            INK,
        );
        page.rule(380.0, RIGHT, y + 9.0);
        page.text(
            ctx,
            Weight::Bold,
            "Amount Due",
            (380.0, y + 30.0),
            13.0,
            INK,
        );
        page.text_right(
            ctx,
            Weight::Bold,
            &format_money(subtotal_cents),
            (RIGHT, y + 30.0),
            13.0,
            INK,
        );
        page.text(ctx, Weight::Regular, "USD", (380.0, y + 46.0), SMALL, MUTED);
        self.y = y + 68.0;
    }

    fn section(&mut self, label: &str, text: &str) {
        let ctx = self.ctx;
        let lines = ctx.wrap(Weight::Regular, text, RIGHT - LEFT, BODY);
        self.ensure(LINE_HEIGHT * 2.0 + 24.0, false);
        let y = self.y;
        self.page()
            .text(ctx, Weight::Regular, label, (LEFT, y), SMALL, MUTED);
        self.y += 17.0;
        for line in lines {
            self.ensure(LINE_HEIGHT, false);
            let y = self.y;
            self.page()
                .text(ctx, Weight::Regular, &line, (LEFT, y), BODY, INK);
            self.y += LINE_HEIGHT;
        }
        self.y += 14.0;
    }
}

/// Fits the logo inside its box, keeping aspect ratio, at the page's top left.
fn place_logo(page: &mut Page, id: &XObjectId, px: (usize, usize)) -> f32 {
    let (w, h) = (px.0 as f32, px.1 as f32);
    let scale = (LOGO_MAX_WIDTH / w).min(LOGO_MAX_HEIGHT / h);
    let (width, height) = (w * scale, h * scale);
    page.ops.push(Op::UseXobject {
        id: id.clone(),
        transform: XObjectTransform {
            translate_x: Some(Pt(LEFT)),
            translate_y: Some(Pt(PAGE_HEIGHT - 54.0 - height)),
            // At 72 dpi one pixel is one point; pick the dpi that yields `width`.
            dpi: Some(72.0 * w / width),
            ..XObjectTransform::default()
        },
    });
    height
}

pub fn render(invoice: &PaperInvoice) -> Result<Vec<u8>, String> {
    if invoice.lines.is_empty() {
        return Err("An invoice needs at least one line".into());
    }
    let title = invoice
        .number
        .clone()
        .unwrap_or_else(|| "Draft invoice".into());
    let mut doc = PdfDocument::new(&title);
    let fonts = fonts()?;
    let ctx = Ctx {
        regular: doc.add_font(&fonts.regular),
        bold: doc.add_font(&fonts.bold),
        fonts,
    };

    let draft = invoice.number.is_none();
    let mut first = Page::new();
    if draft {
        first.watermark(&ctx);
    }

    // Header: optional logo, title and dates on the left, Bill To on the right.
    let mut header_top = 54.0;
    if let Some(bytes) = &invoice.logo {
        let image = RawImage::decode_from_bytes(bytes, &mut Vec::new())
            .map_err(|e| format!("Could not decode the logo image: {e}"))?;
        let dims = (image.width, image.height);
        if dims.0 == 0 || dims.1 == 0 {
            return Err("The logo image is empty".into());
        }
        let id = doc.add_image(&image);
        header_top += place_logo(&mut first, &id, dims) + 26.0;
    }

    let title_y = header_top + 26.0;
    first.text(&ctx, Weight::Bold, "Invoice", (LEFT, title_y), 26.0, INK);
    let number_label = invoice.number.clone().unwrap_or_else(|| "DRAFT".into());
    let due_value = format!("{} ({})", invoice.due_date, invoice.terms);
    let meta = [
        ("Invoice: ", number_label.as_str()),
        ("Issue Date: ", invoice.issue_date.as_str()),
        ("Due Date: ", due_value.as_str()),
    ];
    let mut left_y = title_y + 26.0;
    for (label, value) in meta {
        first.text(&ctx, Weight::Regular, label, (LEFT, left_y), BODY, MUTED);
        let x = LEFT + ctx.width(Weight::Regular, label, BODY);
        first.text(&ctx, Weight::Regular, value, (x, left_y), BODY, INK);
        left_y += 15.0;
    }
    if let Some(subject) = &invoice.subject {
        left_y += 6.0;
        for line in ctx.wrap(Weight::Bold, subject, 260.0, 11.0) {
            first.text(&ctx, Weight::Bold, &line, (LEFT, left_y), 11.0, INK);
            left_y += 15.0;
        }
    }

    let mut right_y = header_top + 10.0;
    first.text_right(
        &ctx,
        Weight::Regular,
        "BILL TO",
        (RIGHT, right_y),
        SMALL,
        MUTED,
    );
    right_y += 19.0;
    for line in ctx.wrap(Weight::Bold, &invoice.bill_to_name, 220.0, 12.5) {
        first.text_right(&ctx, Weight::Bold, &line, (RIGHT, right_y), 12.5, INK);
        right_y += 16.0;
    }
    for line in invoice
        .bill_to_lines
        .iter()
        .flat_map(|l| ctx.wrap(Weight::Regular, l, 220.0, BODY))
    {
        first.text_right(&ctx, Weight::Regular, &line, (RIGHT, right_y), BODY, INK);
        right_y += LINE_HEIGHT;
    }

    let mut y = left_y.max(right_y) + 26.0;
    first.text(&ctx, Weight::Regular, "FROM", (LEFT, y), SMALL, MUTED);
    y += 19.0;
    for line in ctx.wrap(Weight::Bold, &invoice.issuer_name, 300.0, 12.0) {
        first.text(&ctx, Weight::Bold, &line, (LEFT, y), 12.0, INK);
        y += 16.0;
    }
    for line in invoice
        .issuer_lines
        .iter()
        .flat_map(|l| ctx.wrap(Weight::Regular, l, 300.0, BODY))
    {
        first.text(&ctx, Weight::Regular, &line, (LEFT, y), BODY, INK);
        y += LINE_HEIGHT;
    }

    let mut flow = Flow {
        ctx: &ctx,
        draft,
        number: invoice.number.clone().unwrap_or_else(|| "DRAFT".into()),
        pages: vec![first],
        y: y + 34.0,
    };
    // A very tall header leaves no room for the table; start it on page two.
    if flow.y > CONTENT_BOTTOM - 80.0 {
        flow.new_page(true);
    } else {
        flow.table_header();
    }
    for line in &invoice.lines {
        flow.row(line);
    }
    flow.totals(invoice.subtotal_cents);
    if !invoice.splits.is_empty() {
        flow.section("TIME BY PROJECT", &invoice.splits.join("\n"));
    }
    if let Some(text) = &invoice.payment_instructions {
        flow.section("PAYMENT INSTRUCTIONS", text);
    }
    if let Some(text) = &invoice.notes {
        flow.section("NOTES", text);
    }

    let mut pages = flow.pages;
    let count = pages.len();
    for (index, page) in pages.iter_mut().enumerate() {
        page.rule(LEFT, RIGHT, FOOTER_RULE);
        page.text(
            &ctx,
            Weight::Regular,
            &invoice.issuer_name,
            (LEFT, FOOTER_TEXT),
            SMALL,
            MUTED,
        );
        page.text_right(
            &ctx,
            Weight::Regular,
            &format!("Page {} of {count}", index + 1),
            (RIGHT, FOOTER_TEXT),
            SMALL,
            MUTED,
        );
    }

    let pdf_pages = pages
        .into_iter()
        .map(|page| PdfPage::new(Mm(215.9), Mm(279.4), page.ops))
        .collect();
    Ok(doc
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new()))
}
