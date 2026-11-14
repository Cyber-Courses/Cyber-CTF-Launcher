//! What the app downloaded for its own operations (`machine::storage::owned`): the images it
//! pulled and the boxes it downloaded.

use rusqlite::Connection;

use crate::error::Result;
use crate::machine::storage::owned::{Owned, OwnedBox, OwnedImage};

/// The whole record.
///
/// # Errors
///
/// The store can't be read.
pub(crate) fn load(conn: &Connection) -> Result<Owned> {
    let mut images = conn.prepare("SELECT reference, image_id FROM storage_images")?;
    let images = images.query_map([], |row| Ok(OwnedImage { reference: row.get(0)?, id: row.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
    let mut boxes = conn.prepare("SELECT name, provider, version FROM storage_boxes")?;
    let boxes = boxes.query_map([], |row| Ok(OwnedBox { name: row.get(0)?, provider: row.get(1)?, version: row.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
    Ok(Owned { images, boxes })
}

/// Saves `owned` as the whole record.
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn replace(conn: &Connection, owned: &Owned) -> Result<()> {
    conn.execute("DELETE FROM storage_images", [])?;
    conn.execute("DELETE FROM storage_boxes", [])?;
    add(conn, owned)
}

/// Adds `owned`'s images and boxes to the record (one already there stays once).
///
/// # Errors
///
/// The store can't be written.
pub(crate) fn add(conn: &Connection, owned: &Owned) -> Result<()> {
    let mut image = conn.prepare("INSERT OR IGNORE INTO storage_images (reference, image_id) VALUES (?1, ?2)")?;
    for i in &owned.images {
        image.execute((&i.reference, &i.id))?;
    }
    let mut boxes = conn.prepare("INSERT OR IGNORE INTO storage_boxes (name, provider, version) VALUES (?1, ?2, ?3)")?;
    for b in &owned.boxes {
        boxes.execute((&b.name, &b.provider, &b.version))?;
    }
    Ok(())
}
