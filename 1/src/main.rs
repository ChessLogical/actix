use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, HttpResponse, HttpServer, Result};
use futures_util::stream::StreamExt as _;
use uuid::Uuid;
use std::io::Write;
use actix_web::web::Data;

const MAX_SIZE: usize = 20 * 1024 * 1024; // 20 MB

async fn save_file(mut payload: Multipart) -> Result<HttpResponse> {
    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_disposition = field.content_disposition();
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
        }
    }

    Ok(HttpResponse::SeeOther().append_header(("Location", "/")).finish())
}

async fn index() -> Result<HttpResponse> {
    let mut paths = std::fs::read_dir("./static")
        .unwrap()
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, std::io::Error>>()
        .unwrap();

    // Sort paths by modification time to show the latest files first
    paths.sort_by_key(|path| std::fs::metadata(path).unwrap().modified().unwrap());
    paths.reverse();

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

    for path in paths {
        let file_path = path.to_str().unwrap();
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
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
