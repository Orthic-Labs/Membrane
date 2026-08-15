use tauri::Manager;

/// Opens a product tab as a Tauri webview window.
/// Each product registers its live surface over inherited lifecycle transport;
/// for now we open a generic window per product and load its installRoot-adjacent UI.
/// The Hub hosts tabs; it does not render product content itself (D-2).
pub fn open_product_tab(app: &tauri::AppHandle, product_id: &str, url: &str) -> Result<(), String> {
    let label = format!("product-{}", product_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let _window = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(url.parse().map_err(|e| format!("{e}"))?))
        .title(format!("Orthic — {}", product_id))
        .inner_size(900.0, 600.0)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn close_product_tab(app: &tauri::AppHandle, product_id: &str) -> Result<(), String> {
    let label = format!("product-{}", product_id);
    if let Some(win) = app.get_webview_window(&label) {
        win.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn label_format() {
        assert_eq!(format!("product-{}", "cortex"), "product-cortex");
    }
}
