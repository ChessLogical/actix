use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, HttpResponse, HttpServer, Result};
use futures_util::stream::StreamExt as _;
use uuid::Uuid;
use std::io::Write;
use actix_web::web::Data;
use rusqlite::{params, Connection, Result as SqlResult};
use std::sync::Mutex;

// Maximum file size (20 MB)
const MAX_SIZE: usize = 20 * 1024 * 1024;

async fn save_file(mut payload: Multipart, conn: web::Data<Mutex<Connection>>) -> Result<HttpResponse> {
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_disposition = field.content_disposition().clone();
        let filename = content_disposition.get_filename().unwrap_or("unknown");
        let file_extension = filename.split('.').last().unwrap_or("");

        let valid_image_extensions = ["jpg", "jpeg", "png", "gif", "webp"];
        let valid_video_extensions = ["mp4", "mp3", "webm"];

        if valid_image_extensions.contains(&file_extension) || valid_video_extensions.contains(&file_extension) {
            let uuid = Uuid::new_v4();
            let file_path = format!("./static/{}-{}", uuid, sanitize_filename::sanitize(&filename));
            let file_path_clone = file_path.clone();
            let mut f = web::block(move || std::fs::File::create(file_path_clone)).await??;

            while let Some(chunk) = field.next().await {
                let data = chunk?;
                f = web::block(move || f.write_all(&data).map(|_| f)).await??;
            }

            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO files (uuid, filename, path) VALUES (?1, ?2, ?3)",
                params![uuid.to_string(), filename, file_path],
            ).unwrap();
        }
    }

    Ok(HttpResponse::SeeOther().append_header(("Location", "/")).finish())
}

async fn index(conn: web::Data<Mutex<Connection>>) -> Result<HttpResponse> {
    let conn = conn.lock().unwrap();
    let mut stmt = conn.prepare("SELECT path FROM files ORDER BY id DESC").unwrap();
    let paths = stmt.query_map([], |row| row.get(0)).unwrap();

    let mut body = String::new();

    body.push_str("<html><head><title>File Upload</title><style>");
    body.push_str(r#"
        body {
            background-color: #121212;
            color: #FFFFFF;
            font-family: Arial, sans-serif;
        }
        form {
            margin-bottom: 20px;
        }
        .post {
            border-bottom: 5px solid #333333;
            padding: 10px 0;
        }
        img, video {
            max-width: 200px;
            max-height: 200px;
            display: block;
            margin-bottom: 10px;
        }
    "#);
    body.push_str("</style></head><body>");
    body.push_str(
        r#"<form action="/upload" method="post" enctype="multipart/form-data">
            <input type="file" name="file" multiple>
            <button type="submit">Upload</button>
        </form>"#,
    );

    for path_result in paths {
        let file_path: String = path_result.unwrap();
        body.push_str("<div class=\"post\">");
        if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") || file_path.ends_with(".png") || file_path.ends_with(".gif") || file_path.ends_with(".webp") {
            body.push_str(&format!(r#"<img src="{}"><br>"#, file_path));
        } else if file_path.ends_with(".mp4") || file_path.ends_with(".mp3") || file_path.ends_with(".webm") {
            body.push_str(&format!(r#"<video controls><source src="{}"></video><br>"#, file_path));
        }
        body.push_str("</div>");
    }

    body.push_str("</body></html>");

    Ok(HttpResponse::Ok().content_type("text/html").body(body))
}

fn initialize_db() -> SqlResult<Connection> {
    let conn = Connection::open("my_database.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uuid TEXT NOT NULL,
            filename TEXT NOT NULL,
            path TEXT NOT NULL
        )",
        [],
    )?;
    Ok(conn)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let conn = initialize_db().unwrap();
    let conn_data = Data::new(Mutex::new(conn));

    HttpServer::new(move || {
        App::new()
            .app_data(conn_data.clone())
            .app_data(Data::new(web::JsonConfig::default().limit(MAX_SIZE)))
            .service(
                web::resource("/")
                    .route(web::get().to(index))
            )
            .service(
                web::resource("/upload")
                    .route(web::post().to(save_file))
            )
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}

