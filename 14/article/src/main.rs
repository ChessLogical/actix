use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, HttpResponse, HttpServer, Result};
use futures_util::stream::StreamExt as _;
use mysql_async::prelude::*;
use mysql_async::Pool;
use rand::{distributions::Alphanumeric, Rng};
use std::collections::HashMap;
use std::fs::read_to_string;
use std::io::Write;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use log::info;
use env_logger;

// Sanitize board names to remove any unwanted characters
fn sanitize_board_name(board_name: &str) -> String {
    board_name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

async fn save_file(
    mut payload: Multipart,
    pool: web::Data<Pool>,
    board_name: web::Path<String>,
) -> Result<HttpResponse, actix_web::Error> {
    let board_name = sanitize_board_name(&board_name.into_inner());
    let table_name = format!("{}_files", board_name);

    let mut title = String::new();
    let mut message = String::new();
    let mut file_path = None;
    let mut parent_id: i32 = 0;

    while let Some(item) = payload.next().await {
        let mut field = item.map_err(actix_web::error::ErrorInternalServerError)?;
        let content_disposition = field.content_disposition().clone();
        let name = content_disposition.get_name().unwrap_or("").to_string();

        match name.as_str() {
            "title" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.map_err(actix_web::error::ErrorInternalServerError)?;
                    title.push_str(&String::from_utf8_lossy(&data));
                }
            },
            "message" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.map_err(actix_web::error::ErrorInternalServerError)?;
                    message.push_str(&String::from_utf8_lossy(&data));
                }
            },
            "file" => {
                if let Some(filename) = content_disposition.get_filename() {
                    let file_extension = filename.split('.').last().unwrap_or("");
                    let sanitized_filename = sanitize_filename::sanitize(&filename);
                    let unique_id: String = rand::thread_rng()
                        .sample_iter(&Alphanumeric)
                        .take(5)
                        .map(char::from)
                        .collect();
                    let unique_filename = format!("{}-{}", unique_id, sanitized_filename);

                    let valid_image_extensions = ["jpg", "jpeg", "png", "gif", "webp"];
                    let valid_video_extensions = ["mp4", "mp3", "webm"];

                    if valid_image_extensions.contains(&file_extension) || valid_video_extensions.contains(&file_extension) {
                        let file_path_string = format!("./static/{}", unique_filename);
                        let file_path_clone = file_path_string.clone();
                        let f = web::block(move || std::fs::File::create(file_path_clone)).await.map_err(actix_web::error::ErrorInternalServerError)??;

                        while let Some(chunk) = field.next().await {
                            let data = chunk.map_err(actix_web::error::ErrorInternalServerError)?;
                            web::block({
                                let mut f = f.try_clone().map_err(actix_web::error::ErrorInternalServerError)?;
                                move || {
                                    f.write_all(&data)?;
                                    Ok::<_, std::io::Error>(())
                                }
                            }).await.map_err(actix_web::error::ErrorInternalServerError)??;
                        }

                        file_path = Some(file_path_string);
                    }
                }
            },
            "parent_id" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.map_err(actix_web::error::ErrorInternalServerError)?;
                    parent_id = String::from_utf8_lossy(&data).trim().parse().unwrap_or(0);
                }
            },
            _ => {},
        }
    }

    if title.trim().is_empty() || message.trim().is_empty() {
        return Ok(HttpResponse::BadRequest().body("Title and message are mandatory."));
    }

    if title.len() > 30 || message.len() > 50000 {
        return Ok(HttpResponse::BadRequest().body("Title or message is too long."));
    }

    let post_id: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(5)
        .map(char::from)
        .collect();

    let mut conn = pool.get_conn().await.map_err(actix_web::error::ErrorInternalServerError)?;
    let query = format!(
        "INSERT INTO {} (post_id, parent_id, title, message, file_path) VALUES (?, ?, ?, ?, ?)",
        table_name
    );
    info!("Executing query: {}", query);
    conn.exec_drop(&query, (post_id.clone(), parent_id, title, message, file_path))
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if parent_id != 0 {
        let update_query = format!(
            "UPDATE {} SET last_reply_at = CURRENT_TIMESTAMP WHERE id = ? OR parent_id = ?",
            table_name
        );
        info!("Executing query: {}", update_query);
        conn.exec_drop(&update_query, (parent_id, parent_id))
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;
    }

    if parent_id == 0 {
        Ok(HttpResponse::SeeOther().append_header(("Location", format!("/{}", board_name))).finish())
    } else {
        Ok(HttpResponse::SeeOther().append_header(("Location", format!("/{}/post/{}", board_name, parent_id))).finish())
    }
}

async fn view_post(
    pool: web::Data<Pool>,
    path: web::Path<(String, i32)>,
) -> Result<HttpResponse, actix_web::error::Error> {
    let (board_name, post_id) = path.into_inner();
    let board_name = sanitize_board_name(&board_name);
    let table_name = format!("{}_files", board_name);

    let mut conn = pool.get_conn().await.map_err(actix_web::error::ErrorInternalServerError)?;
    let query = format!(
        "SELECT id, post_id, parent_id, title, message, file_path FROM {} WHERE id = ? OR parent_id = ? ORDER BY id ASC",
        table_name
    );
    info!("Executing query: {}", query);
    let posts: Vec<(i32, String, i32, String, String, Option<String>)> = conn.exec(&query, (post_id, post_id))
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let mut posts_html = String::new();
    let mut is_original_post = true;
    let mut reply_count = 1;

    for (_, post_id, _, title, message, file_path) in posts {
        let post_color = generate_color_from_id(&post_id);
        posts_html.push_str(&format!("<div class=\"post\" style=\"border-color: {}\">", post_color));
        if is_original_post {
            posts_html.push_str("<div class=\"post-id\">Original Post</div>");
            is_original_post = false;
        } else {
            posts_html.push_str(&format!("<div class=\"post-id\">Reply {}</div>", reply_count));
            reply_count += 1;
        }
        posts_html.push_str(&format!("<div class=\"post-title\">{}</div>", title));
        if let Some(file_path) = file_path {
            if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") || file_path.ends_with(".png") || file_path.ends_with(".gif") || file_path.ends_with(".webp") {
                posts_html.push_str(&format!(r#"<img src="/static/{}"><br>"#, file_path.trim_start_matches("./static/")));
            } else if file_path.ends_with(".mp4") || file_path.ends_with(".mp3") || file_path.ends_with(".webm") {
                posts_html.push_str(&format!(r#"<video controls><source src="/static/{}"></video><br>"#, file_path.trim_start_matches("./static/")));
            }
        }
        posts_html.push_str(&format!("<div class=\"post-message\">{}</div>", message));
        posts_html.push_str("</div>");
    }

    let mut context = HashMap::new();
    context.insert("PARENT_ID", post_id.to_string());
    context.insert("POSTS", posts_html);
    context.insert("BOARD_NAME", format!("/{}", board_name));

    let body = render_template("templates/view_post.html", &context);

    Ok(HttpResponse::Ok().content_type("text/html").body(body))
}

async fn index() -> Result<HttpResponse, actix_web::Error> {
    match read_to_string("/home/article/index.html") {
        Ok(body) => Ok(HttpResponse::Ok().content_type("text/html").body(body)),
        Err(_) => Ok(HttpResponse::NotFound().body("index.html not found")),
    }
}

async fn board_index(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, actix_web::error::Error> {
    let board_name = sanitize_board_name(&path.into_inner());
    let table_name = format!("{}_files", board_name);
    let page: usize = query.get("page").and_then(|p| p.parse().ok()).unwrap_or(1);
    let offset = (page - 1) * 30;

    let mut conn = pool.get_conn().await.map_err(actix_web::error::ErrorInternalServerError)?;

    let total_posts_query = format!("SELECT COUNT(*) FROM {} WHERE parent_id = 0", table_name);
    info!("Executing query: {}", total_posts_query);
    let total_posts: i64 = conn.exec_first(&total_posts_query, ()).await.map_err(actix_web::error::ErrorInternalServerError)?.unwrap_or(0);

    let total_pages = (total_posts as f64 / 30.0).ceil() as usize;

    let posts_query = format!(
        "SELECT id, post_id, title, message, file_path FROM {} WHERE parent_id = 0 ORDER BY last_reply_at DESC LIMIT 30 OFFSET ?",
        table_name
    );
    info!("Executing query: {}", posts_query);
    let posts: Vec<(i32, String, String, String, Option<String>)> = conn.exec(&posts_query, (offset as i64,)).await.map_err(actix_web::error::ErrorInternalServerError)?;

    let mut posts_html = String::new();

    for (id, post_id, title, message, file_path) in posts {
        let reply_count_query = format!("SELECT COUNT(*) FROM {} WHERE parent_id = ?", table_name);
        info!("Executing query: {}", reply_count_query);
        let reply_count: i32 = conn.exec_first(&reply_count_query, (id,)).await.map_err(actix_web::error::ErrorInternalServerError)?.unwrap_or(0);

        let truncated_message = if message.len() > 2700 {
            format!("{}... <a href=\"/{}/post/{}\" class=\"view-full-post\">Click here to open full post</a>", &message[..2700], board_name, id)
        } else {
            message.clone()
        };

        let post_color = generate_color_from_id(&post_id);

        posts_html.push_str("<div class=\"post\">");
        posts_html.push_str(&format!("<div class=\"post-id-box\" style=\"background-color: {}\">{}</div>", post_color, post_id));
        posts_html.push_str(&format!("<div class=\"post-title title-green\">{}</div>", title));
        if let Some(file_path) = file_path {
            if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") || file_path.ends_with(".png") || file_path.ends_with(".gif") || file_path.ends_with(".webp") {
                posts_html.push_str(&format!(r#"<img src="/static/{}"><br>"#, file_path.trim_start_matches("./static/")));
            } else if file_path.ends_with(".mp4") || file_path.ends_with(".mp3") || file_path.ends_with(".webm") {
                posts_html.push_str(&format!(r#"<video controls><source src="/static/{}"></video><br>"#, file_path.trim_start_matches("./static/")));
            }
        }
        posts_html.push_str(&format!("<div class=\"post-message\">{}</div>", truncated_message));
        posts_html.push_str(&format!("<a class=\"reply-button\" href=\"/{}/post/{}\">Reply ({})</a>", board_name, id, reply_count));
        posts_html.push_str("</div>");
    }

    let mut pagination_html = String::new();
    if page > 1 {
        let prev_page = page - 1;
        pagination_html.push_str(&format!(r#"<a href="/{}?page={}">Previous</a>"#, board_name, prev_page));
    }
    if page < total_pages {
        let next_page = page + 1;
        pagination_html.push_str(&format!(r#"<a href="/{}?page={}">Next</a>"#, board_name, next_page));
    }

    let mut context = HashMap::new();
    context.insert("POSTS", posts_html);
    context.insert("PAGINATION", pagination_html);
    context.insert("BOARD_NAME", format!("/{}", board_name));

    let body = render_template("templates/index.html", &context);

    Ok(HttpResponse::Ok().content_type("text/html").body(body))
}

fn render_template(path: &str, context: &HashMap<&str, String>) -> String {
    let template = read_to_string(path).expect("Unable to read template file");
    let mut rendered = template;
    for (key, value) in context {
        let placeholder = format!("{{{{{}}}}}", key);
        rendered = rendered.replace(&placeholder, value);
    }
    rendered
}

fn generate_color_from_id(id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let hash = hasher.finish();

    let r = (hash & 0xFF) as u8;
    let g = ((hash >> 8) & 0xFF) as u8;
    let b = ((hash >> 16) & 0xFF) as u8;

    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init(); // Initialize logging
    let database_url = "mysql://my_user:my_password@localhost/my_database";
    let pool = Pool::new(database_url);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .service(
                web::resource("/")
                    .route(web::get().to(index))
            )
            .service(
                web::resource("/{board_name}")
                    .route(web::get().to(board_index))
                    .route(web::post().to(save_file))
            )
            .service(
                web::resource("/{board_name}/post/{id}")
                    .route(web::get().to(view_post))
            )
            .service(fs::Files::new("/static", "./static").show_files_listing())
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
