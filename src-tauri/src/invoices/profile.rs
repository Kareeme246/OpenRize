//! The issuer: the identity every invoice is sent "From". One local profile;
//! its logo is validated and normalized on import so a stored or embedded image
//! is always something the PDF renderer can decode.

use chrono::Datelike;
use image::{imageops::FilterType, ImageFormat, ImageReader};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

const MAX_LOGO_UPLOAD_BYTES: usize = 5 * 1024 * 1024;
/// Decoded-pixel guard against decompression bombs (a tiny file, a huge canvas).
const MAX_LOGO_SOURCE_PIXELS: u64 = 40_000_000;
/// Stored logos fit this box: 4x the paper's 180 x 56 pt slot, so they stay
/// sharp when zoomed or printed without bloating every archived PDF.
const LOGO_BOX: (u32, u32) = (720, 224);
const MAX_NAME: usize = 120;
const MAX_ADDRESS: usize = 500;
const MAX_SHORT: usize = 120;
const MAX_TEXT: usize = 2000;
pub const MAX_TERMS_DAYS: i64 = 365;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceProfile {
    pub name: String,
    pub address: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub payment_instructions: Option<String>,
    pub default_notes: Option<String>,
    pub default_terms_days: i64,
    pub has_logo: bool,
    /// The year invoices are currently numbered in, and the next number in it.
    pub number_year: i32,
    pub next_number: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub name: String,
    pub address: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub payment_instructions: Option<String>,
    pub default_notes: Option<String>,
    pub default_terms_days: i64,
    /// Sets the next number for the current year; omitted leaves it alone.
    pub next_number: Option<i64>,
}

/// What Finalize snapshots of the issuer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuerSnapshot {
    pub name: String,
    pub address: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

pub struct Issuer {
    pub snapshot: IssuerSnapshot,
    pub logo: Option<Vec<u8>>,
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn optional(value: Option<String>, max: usize, label: &str) -> Result<Option<String>, String> {
    let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    if value.as_ref().is_some_and(|v| v.chars().count() > max) {
        return Err(format!("{label} is too long"));
    }
    Ok(value)
}

pub fn current_year() -> i32 {
    chrono::Local::now().year()
}

pub fn next_number_for(conn: &Connection, year: i32) -> Result<i64, String> {
    Ok(conn
        .query_row(
            "SELECT next_number FROM invoice_sequences WHERE year = ?1",
            [year],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?
        .unwrap_or(1))
}

pub fn get(conn: &Connection) -> Result<InvoiceProfile, String> {
    let year = current_year();
    let next_number = next_number_for(conn, year)?;
    conn.query_row(
        "SELECT name, address, email, phone, payment_instructions, default_notes,
                default_terms_days, logo IS NOT NULL
         FROM invoice_profile WHERE id = 1",
        [],
        |row| {
            Ok(InvoiceProfile {
                name: row.get(0)?,
                address: row.get(1)?,
                email: row.get(2)?,
                phone: row.get(3)?,
                payment_instructions: row.get(4)?,
                default_notes: row.get(5)?,
                default_terms_days: row.get(6)?,
                has_logo: row.get(7)?,
                number_year: year,
                next_number,
            })
        },
    )
    .map_err(err)
}

pub fn update(
    conn: &mut Connection,
    input: ProfileInput,
    now: u64,
) -> Result<InvoiceProfile, String> {
    let name = input.name.trim().to_owned();
    if name.chars().count() > MAX_NAME {
        return Err("Business name is too long".into());
    }
    let address = input.address.trim().to_owned();
    if address.chars().count() > MAX_ADDRESS || address.lines().count() > 8 {
        return Err("Address is too long (500 characters, 8 lines)".into());
    }
    if !(0..=MAX_TERMS_DAYS).contains(&input.default_terms_days) {
        return Err("Payment terms must be between 0 and 365 days".into());
    }
    let email = optional(input.email, MAX_SHORT, "Email")?;
    let phone = optional(input.phone, MAX_SHORT, "Phone")?;
    let payment = optional(input.payment_instructions, MAX_TEXT, "Payment instructions")?;
    let notes = optional(input.default_notes, MAX_TEXT, "Notes")?;

    let tx = conn.transaction().map_err(err)?;
    tx.execute(
        "UPDATE invoice_profile SET name = ?1, address = ?2, email = ?3, phone = ?4,
                payment_instructions = ?5, default_notes = ?6, default_terms_days = ?7,
                updated_at = ?8
         WHERE id = 1",
        params![
            name,
            address,
            email,
            phone,
            payment,
            notes,
            input.default_terms_days,
            now as i64
        ],
    )
    .map_err(err)?;
    if let Some(next) = input.next_number {
        set_next_number(&tx, current_year(), next)?;
    }
    tx.commit().map_err(err)?;
    get(conn)
}

/// Moves the counter for `year`. It may only move forward past every number
/// already issued that year, so a number is never reused.
fn set_next_number(conn: &Connection, year: i32, next: i64) -> Result<(), String> {
    if !(1..=99_999).contains(&next) {
        return Err("The next invoice number must be between 1 and 99999".into());
    }
    let prefix = format!("INV-{year}-");
    let highest: Option<i64> = conn
        .query_row(
            "SELECT MAX(CAST(SUBSTR(number, ?2) AS INTEGER)) FROM invoices WHERE number LIKE ?1",
            params![format!("{prefix}%"), prefix.len() as i64 + 1],
            |row| row.get(0),
        )
        .map_err(err)?;
    if let Some(highest) = highest {
        if next <= highest {
            return Err(format!(
                "{prefix}{highest:04} is already issued; choose a higher number"
            ));
        }
    }
    conn.execute(
        "INSERT INTO invoice_sequences (year, next_number) VALUES (?1, ?2)
         ON CONFLICT(year) DO UPDATE SET next_number = excluded.next_number",
        params![year, next],
    )
    .map_err(err)?;
    Ok(())
}

/// The profile as a renderer needs it; `None` while it is too incomplete to
/// put on an invoice.
pub fn issuer(conn: &Connection) -> Result<Option<Issuer>, String> {
    let row = conn
        .query_row(
            "SELECT name, address, email, phone, logo FROM invoice_profile WHERE id = 1",
            [],
            |row| {
                Ok((
                    IssuerSnapshot {
                        name: row.get(0)?,
                        address: row.get(1)?,
                        email: row.get(2)?,
                        phone: row.get(3)?,
                    },
                    row.get::<_, Option<Vec<u8>>>(4)?,
                ))
            },
        )
        .map_err(err)?;
    let (snapshot, logo) = row;
    if snapshot.name.is_empty() || snapshot.address.is_empty() {
        return Ok(None);
    }
    Ok(Some(Issuer { snapshot, logo }))
}

pub fn logo_bytes(conn: &Connection) -> Result<Option<Vec<u8>>, String> {
    conn.query_row("SELECT logo FROM invoice_profile WHERE id = 1", [], |row| {
        row.get(0)
    })
    .map_err(err)
}

/// Validates, downsizes and stores an uploaded PNG or JPEG as a PNG.
pub fn set_logo(conn: &Connection, bytes: &[u8], now: u64) -> Result<(), String> {
    let normalized = normalize_logo(bytes)?;
    conn.execute(
        "UPDATE invoice_profile SET logo = ?1, logo_mime = 'image/png', updated_at = ?2 WHERE id = 1",
        params![normalized, now as i64],
    )
    .map_err(err)?;
    Ok(())
}

pub fn clear_logo(conn: &Connection, now: u64) -> Result<(), String> {
    conn.execute(
        "UPDATE invoice_profile SET logo = NULL, logo_mime = NULL, updated_at = ?1 WHERE id = 1",
        [now as i64],
    )
    .map_err(err)?;
    Ok(())
}

pub fn normalize_logo(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("The logo file is empty".into());
    }
    if bytes.len() > MAX_LOGO_UPLOAD_BYTES {
        return Err("Logos can be at most 5 MB".into());
    }
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(err)?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png) | Some(ImageFormat::Jpeg)
    ) {
        return Err("Choose a PNG or JPEG logo".into());
    }
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "That image could not be read".to_string())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_LOGO_SOURCE_PIXELS {
        return Err("That image is too large; use one under 40 megapixels".into());
    }
    let image = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(err)?
        .decode()
        .map_err(|_| "That image could not be read".to_string())?;
    let image = if image.width() > LOGO_BOX.0 || image.height() > LOGO_BOX.1 {
        image.resize(LOGO_BOX.0, LOGO_BOX.1, FilterType::Lanczos3)
    } else {
        image
    };
    // Always 8 bits per channel: 16-bit sources would bloat every archived PDF.
    let image = if image.color().has_alpha() {
        image::DynamicImage::ImageRgba8(image.to_rgba8())
    } else {
        image::DynamicImage::ImageRgb8(image.to_rgb8())
    };
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|_| "That image could not be converted".to_string())?;
    Ok(out.into_inner())
}
