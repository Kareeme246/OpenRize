use super::*;
use std::path::PathBuf;

const LOGO: &[u8] = include_bytes!("testdata/sample-logo.png");
const HOUR: i64 = 3_600_000;

fn db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::migrations::run_migrations(&mut conn).unwrap();
    conn.execute_batch(
        "INSERT INTO clients (id, name, email, address, default_rate, currency, created_at, updated_at)
           VALUES ('c', 'Acme Corp', 'ap@acme.test', '123 Main St.', 100, 'USD', 0, 0);
         INSERT INTO clients (id, name, default_rate, currency, created_at, updated_at)
           VALUES ('other', 'Other Co', 100, 'USD', 0, 0);
         INSERT INTO projects (id, client_id, name, color, created_at, updated_at)
           VALUES ('p', 'c', 'Swap', '#fff', 0, 0);
         INSERT INTO projects (id, client_id, name, color, hourly_rate, created_at, updated_at)
           VALUES ('q', 'c', 'Swap 2', '#fff', 150, 0, 0);
         INSERT INTO projects (id, client_id, name, color, created_at, updated_at)
           VALUES ('o', 'other', 'Elsewhere', '#fff', 0, 0);",
    )
    .unwrap();
    entry(&conn, "e1", "p", 0, HOUR, "approved", true);
    entry(
        &conn,
        "e2",
        "q",
        2 * HOUR,
        2 * HOUR + 90 * 60_000,
        "approved",
        true,
    );
    conn
}

fn entry(
    conn: &Connection,
    id: &str,
    project: &str,
    start: i64,
    end: i64,
    status: &str,
    billable: bool,
) {
    conn.execute(
        "INSERT INTO time_entries (id, started_at, ended_at, description, project_id, status, billable, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'Work', ?4, ?5, ?6, 0, 0)",
        params![id, start, end, project, status, billable],
    )
    .unwrap();
}

fn complete_profile(conn: &Connection) {
    conn.execute(
        "UPDATE invoice_profile SET name = 'Offline Studios', address = '1 Main St.\nPortland, OR 97201',
                email = 'hi@offline.test' WHERE id = 1",
        [],
    )
    .unwrap();
}

fn time(entry: &str) -> LineInput {
    LineInput {
        kind: "time".into(),
        entry_id: Some(entry.into()),
        description: String::new(),
        quantity_hundredths: None,
        unit: None,
        rate_cents: None,
    }
}

fn retainer(rate: i64) -> LineInput {
    LineInput {
        kind: "retainer".into(),
        entry_id: None,
        description: "Monthly support retainer, September 2026".into(),
        quantity_hundredths: Some(100),
        unit: Some("mo".into()),
        rate_cents: Some(rate),
    }
}

fn draft(id: Option<&str>, lines: Vec<LineInput>) -> DraftInput {
    DraftInput {
        id: id.map(str::to_owned),
        client_id: "c".into(),
        bill_to_name: "Acme Corp".into(),
        bill_to_address: Some("123 Main St.".into()),
        bill_to_email: None,
        issue_date: "2026-09-29".into(),
        terms_days: 30,
        subject: Some("Software".into()),
        notes: None,
        payment_instructions: None,
        lines,
    }
}

fn linked(conn: &Connection, entry: &str) -> Option<String> {
    conn.query_row(
        "SELECT invoice_id FROM time_entries WHERE id = ?1",
        [entry],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn lists_only_approved_billable_uninvoiced_time_for_the_client() {
    let conn = db();
    entry(&conn, "pending", "p", 5 * HOUR, 6 * HOUR, "pending", true);
    entry(
        &conn,
        "unbillable",
        "p",
        5 * HOUR,
        6 * HOUR,
        "approved",
        false,
    );
    entry(
        &conn,
        "elsewhere",
        "o",
        5 * HOUR,
        6 * HOUR,
        "approved",
        true,
    );
    entry(&conn, "late", "p", 90 * HOUR, 91 * HOUR, "approved", true);
    let found = billable_entries(&conn, "c", 0, 10 * HOUR as u64, None).unwrap();
    let ids: Vec<_> = found.iter().map(|e| e.entry_id.as_str()).collect();
    assert_eq!(ids, ["e1", "e2"]);
    assert_eq!(found[0].rate_cents, Some(10_000)); // client fallback
    assert_eq!(found[1].rate_cents, Some(15_000)); // project rate
    assert_eq!(found[1].quantity_hundredths, 150);
    assert_eq!(found[1].amount_cents, Some(22_500));
    assert!(billable_entries(&conn, "c", 5, 5, None).is_err());
}

#[test]
fn hybrid_draft_totals_reserve_time_and_release_it_on_edit_and_delete() {
    let mut conn = db();
    let saved = save_draft(
        &mut conn,
        &draft(None, vec![retainer(200_000), time("e1"), time("e2")]),
        5,
    )
    .unwrap();
    // 2,000.00 retainer + 1h x 100 + 1.5h x 150 = 2,325.00
    assert_eq!(saved.summary.total_cents, 232_500);
    assert_eq!(saved.summary.due_date.as_deref(), Some("2026-10-29"));
    assert_eq!(saved.lines.len(), 3);
    assert_eq!(saved.lines[1].description, "Work");
    assert_eq!(
        linked(&conn, "e1").as_deref(),
        Some(saved.summary.id.as_str())
    );

    // Another draft cannot take reserved time.
    assert!(save_draft(&mut conn, &draft(None, vec![time("e1")]), 6).is_err());
    // Nor can a tracked entry appear twice.
    assert!(quote(
        &conn,
        &draft(Some(&saved.summary.id), vec![time("e1"), time("e1")])
    )
    .is_err());

    // Dropping a line releases exactly that entry.
    let edited = save_draft(
        &mut conn,
        &draft(Some(&saved.summary.id), vec![retainer(200_000), time("e2")]),
        7,
    )
    .unwrap();
    assert_eq!(edited.summary.total_cents, 222_500);
    assert_eq!(linked(&conn, "e1"), None);
    assert!(linked(&conn, "e2").is_some());

    // Reserved time is locked against edits.
    assert!(conn
        .execute("UPDATE time_entries SET ended_at = 10 WHERE id = 'e2'", [])
        .is_err());

    delete_draft(&mut conn, &saved.summary.id).unwrap();
    assert_eq!(linked(&conn, "e2"), None);
    assert!(list(&conn).unwrap().is_empty());
}

#[test]
fn a_time_line_keeps_its_entry_but_takes_an_edited_rate_and_description() {
    let mut conn = db();
    let mut line = time("e1");
    line.rate_cents = Some(12_345);
    line.description = "Interface design".into();
    let saved = save_draft(&mut conn, &draft(None, vec![line]), 5).unwrap();
    assert_eq!(saved.lines[0].rate_cents, 12_345);
    assert_eq!(saved.lines[0].description, "Interface design");
    assert_eq!(saved.lines[0].amount_cents, 12_345);
    // The snapshot survives a later rate change.
    conn.execute("UPDATE clients SET default_rate = 999 WHERE id = 'c'", [])
        .unwrap();
    assert_eq!(
        get(&conn, &saved.summary.id).unwrap().lines[0].rate_cents,
        12_345
    );
}

#[test]
fn validation_rejects_bad_drafts() {
    let mut conn = db();
    let ok = draft(None, vec![retainer(1000)]);
    let bad = |mutate: &dyn Fn(&mut DraftInput)| {
        let mut input = ok.clone();
        mutate(&mut input);
        quote(&conn, &input).is_err()
    };
    assert!(bad(&|d| d.lines.clear()));
    assert!(bad(&|d| d.client_id = "nope".into()));
    assert!(bad(&|d| d.bill_to_name = "  ".into()));
    assert!(bad(&|d| d.issue_date = "09/29/2026".into()));
    assert!(bad(&|d| d.terms_days = 366));
    assert!(bad(&|d| d.lines[0].description = " ".into()));
    assert!(bad(&|d| d.lines[0].quantity_hundredths = Some(0)));
    assert!(bad(&|d| d.lines[0].rate_cents = Some(-1)));
    assert!(bad(&|d| d.lines[0].kind = "tax".into()));
    assert!(bad(&|d| d.lines[0].unit = Some("x".repeat(17))));
    assert!(bad(&|d| d.subject = Some("x".repeat(201))));
    // Time from another client's project, unapproved or unbillable time.
    entry(
        &conn,
        "elsewhere",
        "o",
        5 * HOUR,
        6 * HOUR,
        "approved",
        true,
    );
    entry(&conn, "pending", "p", 5 * HOUR, 6 * HOUR, "pending", true);
    assert!(bad(&|d| d.lines = vec![time("elsewhere")]));
    assert!(bad(&|d| d.lines = vec![time("pending")]));
    assert!(bad(&|d| d.lines = vec![time("missing")]));
    // A missing rate is a clear error, not a $0 line.
    conn.execute("UPDATE clients SET default_rate = NULL WHERE id = 'c'", [])
        .unwrap();
    let error = quote(&conn, &draft(None, vec![time("e1")])).unwrap_err();
    assert!(error.contains("hourly rate"), "{error}");
    // Editing a finalized invoice is refused.
    conn.execute("UPDATE clients SET default_rate = 100 WHERE id = 'c'", [])
        .unwrap();
    complete_profile(&conn);
    let saved = save_draft(&mut conn, &draft(None, vec![retainer(1000)]), 1).unwrap();
    finalize(&mut conn, &saved.summary.id, 2).unwrap();
    assert!(save_draft(
        &mut conn,
        &draft(Some(&saved.summary.id), vec![retainer(2000)]),
        3
    )
    .is_err());
}

#[test]
fn finalize_numbers_invoices_by_year_and_archives_the_pdf() {
    let mut conn = db();
    complete_profile(&conn);
    let first = save_draft(&mut conn, &draft(None, vec![time("e1")]), 1).unwrap();
    let done = finalize(&mut conn, &first.summary.id, 10).unwrap();
    assert_eq!(done.summary.number.as_deref(), Some("INV-2026-0001"));
    assert_eq!(done.summary.status, "open");
    assert_eq!(done.summary.issued_at, Some(10));
    let bytes = stored_pdf(&conn, &done.summary.id).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));

    let mut later = draft(None, vec![time("e2")]);
    later.issue_date = "2026-12-31".into();
    let second = save_draft(&mut conn, &later, 11).unwrap();
    assert_eq!(
        finalize(&mut conn, &second.summary.id, 12)
            .unwrap()
            .summary
            .number
            .as_deref(),
        Some("INV-2026-0002")
    );

    let mut next_year = draft(None, vec![retainer(50_000)]);
    next_year.issue_date = "2027-01-02".into();
    let third = save_draft(&mut conn, &next_year, 13).unwrap();
    assert_eq!(
        finalize(&mut conn, &third.summary.id, 14)
            .unwrap()
            .summary
            .number
            .as_deref(),
        Some("INV-2027-0001")
    );

    // Finalized invoices are frozen: no delete, no re-finalize, PDF bytes unchanged.
    assert!(delete_draft(&mut conn, &first.summary.id).is_err());
    assert!(finalize(&mut conn, &first.summary.id, 15).is_err());
    assert_eq!(stored_pdf(&conn, &first.summary.id).unwrap(), bytes);
}

#[test]
fn finalize_failure_consumes_no_number_and_leaves_the_draft_intact() {
    let mut conn = db();
    let saved = save_draft(&mut conn, &draft(None, vec![time("e1")]), 1).unwrap();
    let error = finalize(&mut conn, &saved.summary.id, 2).unwrap_err();
    assert!(error.contains("Invoice settings"), "{error}");
    assert_eq!(
        get(&conn, &saved.summary.id).unwrap().summary.status,
        "draft"
    );
    assert_eq!(profile::next_number_for(&conn, 2026).unwrap(), 1);
    complete_profile(&conn);
    let zero = save_draft(
        &mut conn,
        &draft(Some(&saved.summary.id), vec![retainer(0)]),
        3,
    )
    .unwrap();
    assert!(finalize(&mut conn, &zero.summary.id, 4).is_err());
    // A number sitting in the way is skipped, never reused.
    conn.execute(
        "UPDATE invoices SET number = 'INV-2026-0001' WHERE id = ?1",
        [&zero.summary.id],
    )
    .unwrap();
    let other = save_draft(&mut conn, &draft(None, vec![retainer(500)]), 5).unwrap();
    assert_eq!(
        finalize(&mut conn, &other.summary.id, 6)
            .unwrap()
            .summary
            .number
            .as_deref(),
        Some("INV-2026-0002")
    );
}

#[test]
fn paid_and_void_transitions_release_time_only_on_void() {
    let mut conn = db();
    complete_profile(&conn);
    let saved = save_draft(&mut conn, &draft(None, vec![time("e1")]), 1).unwrap();
    let id = saved.summary.id.clone();
    assert!(set_paid(&conn, &id, true, 2).is_err()); // still a draft
    assert!(void(&mut conn, &id, 2).is_err());
    finalize(&mut conn, &id, 3).unwrap();
    set_paid(&conn, &id, true, 4).unwrap();
    assert_eq!(get(&conn, &id).unwrap().summary.paid_at, Some(4));
    assert!(void(&mut conn, &id, 5).is_err()); // unpay first
    set_paid(&conn, &id, false, 5).unwrap();
    assert!(linked(&conn, "e1").is_some());
    void(&mut conn, &id, 6).unwrap();
    assert_eq!(get(&conn, &id).unwrap().summary.status, "void");
    assert_eq!(linked(&conn, "e1"), None);
    assert!(stored_pdf(&conn, &id).is_ok());
    // The freed entry can be invoiced again, and the void number is not reused.
    let again = save_draft(&mut conn, &draft(None, vec![time("e1")]), 7).unwrap();
    assert_eq!(
        finalize(&mut conn, &again.summary.id, 8)
            .unwrap()
            .summary
            .number
            .as_deref(),
        Some("INV-2026-0002")
    );
}

#[test]
fn next_number_only_moves_forward_past_issued_numbers() {
    let mut conn = db();
    complete_profile(&conn);
    let year = profile::current_year();
    let input = |next| profile::ProfileInput {
        name: "Offline Studios".into(),
        address: "1 Main St.".into(),
        email: None,
        phone: None,
        payment_instructions: None,
        default_notes: None,
        default_terms_days: 30,
        next_number: Some(next),
    };
    let updated = profile::update(&mut conn, input(42), 1).unwrap();
    assert_eq!(updated.next_number, 42);
    assert!(profile::update(&mut conn, input(0), 2).is_err());
    let mut dated = draft(None, vec![retainer(500)]);
    dated.issue_date = format!("{year}-03-01");
    let saved = save_draft(&mut conn, &dated, 3).unwrap();
    let done = finalize(&mut conn, &saved.summary.id, 4).unwrap();
    assert_eq!(done.summary.number, Some(format!("INV-{year}-0042")));
    assert!(profile::update(&mut conn, input(42), 5).is_err());
    assert_eq!(
        profile::update(&mut conn, input(50), 6)
            .unwrap()
            .next_number,
        50
    );
}

#[test]
fn logos_are_validated_and_normalized() {
    let conn = db();
    profile::set_logo(&conn, LOGO, 1).unwrap();
    assert!(profile::get(&conn).unwrap().has_logo);
    let stored = profile::logo_bytes(&conn).unwrap().unwrap();
    assert!(stored.starts_with(b"\x89PNG"));
    for bad in [&b""[..], b"not an image", b"GIF89a....", &LOGO[..40]] {
        assert!(profile::normalize_logo(bad).is_err());
    }
    assert!(profile::normalize_logo(&vec![0u8; 6 * 1024 * 1024]).is_err());
    profile::clear_logo(&conn, 2).unwrap();
    assert!(!profile::get(&conn).unwrap().has_logo);
}

#[test]
fn file_names_are_filesystem_safe() {
    assert_eq!(
        file_name(Some("INV-2026-0001"), "Acme Corp"),
        "INV-2026-0001-Acme-Corp.pdf"
    );
    assert_eq!(
        file_name(None, "Müller & Sørensen / GmbH"),
        "Draft-Müller-Sørensen-GmbH.pdf"
    );
    assert_eq!(file_name(None, "///"), "Draft-invoice.pdf");
}

fn paper_fixture(lines: Vec<PaperLine>, logo: bool, number: Option<&str>) -> PaperInvoice {
    let subtotal = lines.iter().map(|l| l.amount_cents).sum();
    PaperInvoice {
        number: number.map(str::to_owned),
        issue_date: "September 29, 2026".into(),
        due_date: "October 29, 2026".into(),
        terms: "Net 30".into(),
        subject: Some("Software".into()),
        issuer_name: "Offline Studios".into(),
        issuer_lines: vec![
            "123 Main St.".into(),
            "Portland, OR 97201".into(),
            "hello@offline.test".into(),
        ],
        logo: logo.then(|| profile::normalize_logo(LOGO).unwrap()),
        bill_to_name: "Acme Corp".into(),
        bill_to_lines: vec!["Accounts Payable Contact".into(), "123 Main St.".into()],
        lines,
        subtotal_cents: subtotal,
        payment_instructions: Some("ACH: routing 000000000, account 000000000".into()),
        notes: Some("Thank you for your business.".into()),
    }
}

fn paper_line(description: &str, quantity: i64, unit: &str, rate: i64) -> PaperLine {
    PaperLine {
        description: description.into(),
        detail: Some("Swap · September 12, 2026".into()),
        quantity_hundredths: quantity,
        unit: unit.into(),
        rate_cents: rate,
        amount_cents: money::line_amount_cents(quantity, rate).unwrap(),
    }
}

fn page_count(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf);
    text.match_indices("/Type/Page")
        .filter(|(at, _)| !text[at + "/Type/Page".len()..].starts_with('s'))
        .count()
}

/// Renders the risky specimens. Set `OPENRIZE_INVOICE_SAMPLES` to a directory to
/// keep the PDFs for visual inspection.
#[test]
fn renders_short_long_unicode_and_draft_specimens() {
    let short = pdf::render(&paper_fixture(
        vec![
            paper_line(
                "Interface design · Café São Paulo / München",
                150,
                "hrs",
                20_000,
            ),
            PaperLine {
                detail: None,
                ..paper_line(
                    "Monthly support retainer · September 2026",
                    100,
                    "mo",
                    125_000,
                )
            },
            paper_line("Additional research · Αθήνα / Москва", 33, "hrs", 19_995),
        ],
        true,
        Some("INV-2026-0001"),
    ))
    .unwrap();
    let draft_pdf = pdf::render(&paper_fixture(
        vec![paper_line("Swap", 3100, "", 20_000)],
        false,
        None,
    ))
    .unwrap();
    let long_lines: Vec<PaperLine> = (0..58)
        .map(|i| {
            paper_line(
                &format!("Project planning and technical research, entry {:02} · São Paulo / Zürich / Αθήνα / Москва. A long description of an approved hourly task, including implementation notes and deliverables that must wrap cleanly onto the next line without touching the amount columns.", i + 1),
                if i % 3 == 0 { 75 } else { 150 },
                "hrs",
                12_000 + (i % 4) * 3_500,
            )
        })
        .collect();
    let mut long_invoice = paper_fixture(long_lines, true, Some("INV-2026-0002"));
    long_invoice.bill_to_name = "Müller & Sørensen Consulting Aktiengesellschaft".into();
    long_invoice.bill_to_lines = vec![
        "Attn: Élodie Fournier · Αθήνα / Москва".into(),
        "123 Long Unicode Avenue, Suite 400".into(),
        "Supercalifragilisticexpialidocious-unbroken-token-that-must-still-fit-the-column".into(),
    ];
    long_invoice.notes = Some("Note. ".repeat(400));
    let long = pdf::render(&long_invoice).unwrap();
    let huge_word = pdf::render(&paper_fixture(
        vec![paper_line(&"W".repeat(1900), 100, "hrs", 100)],
        false,
        Some("INV-2026-0003"),
    ))
    .unwrap();

    if let Some(dir) = std::env::var_os("OPENRIZE_INVOICE_SAMPLES") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in [
            ("short", &short),
            ("draft", &draft_pdf),
            ("long", &long),
            ("huge-word", &huge_word),
        ] {
            std::fs::write(dir.join(format!("{name}.pdf")), bytes).unwrap();
        }
    }

    assert_eq!(page_count(&short), 1);
    assert_eq!(page_count(&draft_pdf), 1);
    assert!(
        page_count(&long) >= 5,
        "long invoice should paginate, got {}",
        page_count(&long)
    );
    assert!(page_count(&huge_word) >= 2);
    for bytes in [&short, &draft_pdf, &long, &huge_word] {
        assert!(bytes.starts_with(b"%PDF-"));
    }
    assert!(pdf::render(&paper_fixture(vec![], false, None)).is_err());
}
