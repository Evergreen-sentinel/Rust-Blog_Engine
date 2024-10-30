use actix_files as fs; 
use actix_web::{web, App, HttpServer, HttpResponse};
use reqwest::Error;
use quick_xml::de::from_str;
use serde::{Deserialize, Serialize};
use std::env;
use actix_multipart::Multipart; 
use std::fs as std_fs; 
use std::io::Write; 
use std::path::Path; 
use futures_util::TryStreamExt as _; 
use actix_web::Responder;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use pulldown_cmark::{Parser, html};
use csv::ReaderBuilder;
use std::error::Error as StdError; 

#[derive(Debug, Deserialize, Serialize)]
struct Rss {
    channel: Channel,
}

#[derive(Debug, Deserialize, Serialize)]
struct Channel {
    title: String,
    description: String,
    #[serde(rename = "item")]
    items: Vec<Item>,
}

#[derive(Debug, Deserialize, Serialize, Clone)] 
struct Item {
    title: String,
    link: String,
    description: String,
    category: Option<String>,
}

#[derive(Deserialize)]
struct RssLink {
    url: String,
}

async fn fetch() -> Result<Rss, Box<dyn StdError>> {
    let mut all_items = Vec::new();
    let file_path = "feeds/rss_feeds.csv";
    let mut rdr = ReaderBuilder::new()
        .has_headers(false)
        .from_reader(std_fs::File::open(file_path)?);
    for result in rdr.records() {
        let record = result?;
        for url in record.iter() {
            let url = url.trim(); 
            println!("Fetching RSS feed from URL: {}", url);       
            match reqwest::get(url).await {
                Ok(response) => {
                    if response.status().is_success() {
                        let text = response.text().await?;
                        let mut rss: Rss = from_str(&text)?;                
                        for item in &mut rss.channel.items {
                            item.category = categorize_item(&item.title);
                            all_items.push(item.clone()); 
                        }
                    } else {
                        eprintln!("Failed to fetch {}: HTTP Status {}", url, response.status());
                    }
                }
                Err(err) => {
                    eprintln!("Error fetching {}: {}", url, err); 
                }
            }
        }
    } 
    if all_items.is_empty() {
        eprintln!("No items were collected from any RSS feeds.");
    }
    let combined_rss = Rss {
        channel: Channel {
            title: "Combined RSS Feed".to_string(),
            description: "Aggregated feeds from multiple sources.".to_string(),
            items: all_items,
        },
    };
    Ok(combined_rss)
}

fn categorize_item(title: &str) -> Option<String> {
    let title_lower = title.to_lowercase();
    if title_lower.contains("technology") || title_lower.contains("tech")
        || title_lower.contains("science") || title_lower.contains("innovation")
        || title_lower.contains("software") || title_lower.contains("engineering") {
        Some("Science & Technology".to_string())
    } else if title_lower.contains("business") || title_lower.contains("economy")
        || title_lower.contains("finance") || title_lower.contains("market")
        || title_lower.contains("industry") || title_lower.contains("commerce") {
        Some("Economy".to_string())
    } else if title_lower.contains("sports") || title_lower.contains("athletics")
        || title_lower.contains("competition") || title_lower.contains("tournament")
        || title_lower.contains("championship") || title_lower.contains("game") {
        Some("Sports".to_string())
    } else if title_lower.contains("health") || title_lower.contains("medicine")
        || title_lower.contains("wellness") || title_lower.contains("fitness")
        || title_lower.contains("nutrition") || title_lower.contains("disease") {
        Some("Health".to_string())
    } else if title_lower.contains("literature") || title_lower.contains("books")
        || title_lower.contains("poetry") || title_lower.contains("writing")
        || title_lower.contains("author") || title_lower.contains("novel") {
        Some("Literature".to_string())
    } else if title_lower.contains("art") || title_lower.contains("culture")
        || title_lower.contains("music") || title_lower.contains("painting")
        || title_lower.contains("theater") || title_lower.contains("cinema") {
        Some("Arts & Culture".to_string())
    } else {
        None
    }
}

async fn add_rss_link(rss_link: web::Json<RssLink>) -> impl Responder {
    let file_path = "feeds/rss_feeds.csv";
    let mut file = match std_fs::OpenOptions::new().append(true).open(file_path) {
        Ok(file) => file,
        Err(_) => return HttpResponse::InternalServerError().body("Failed to open RSS feed file"),
    };
    if let Err(_) = writeln!(file, "{}", rss_link.url) {
        return HttpResponse::InternalServerError().body("Failed to write to RSS feed file");
    }
    HttpResponse::Ok().body("RSS link added successfully")
}

async fn rss_handler() -> HttpResponse {
    match fetch().await {
        Ok(rss) => HttpResponse::Ok().json(rss),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

async fn upload_markdown(mut payload: Multipart) -> impl Responder {
    while let Some(item) = payload.try_next().await.unwrap() {
        let mut field = item;
        let content_disposition = field.content_disposition().unwrap();
        if let Some(filename) = content_disposition.get_filename() {
            let filepath = Path::new("blogs").join(filename);
            let mut file = File::create(&filepath).await.unwrap();
            while let Some(chunk) = field.try_next().await.unwrap() {
                file.write_all(&chunk).await.unwrap();
            }
            let markdown_content = tokio::fs::read_to_string(&filepath)
                .await
                .expect("Failed to read file");
            let parser = Parser::new(&markdown_content);
            let mut html_content = String::new();
            html::push_html(&mut html_content, parser);
            let html_filepath = filepath.with_extension("html");
            let mut html_file = File::create(&html_filepath).await.unwrap();
            html_file.write_all(html_content.as_bytes()).await.unwrap();
            return HttpResponse::Found()
                .header("Location", "/") 
                .finish();
        }
    }
    HttpResponse::BadRequest().json("File upload failed")
}

async fn get_uploaded_posts() -> impl Responder {
    let mut posts = Vec::new();
    if let Ok(entries) = std_fs::read_dir("blogs") {
        for entry in entries {
            if let Ok(entry) = entry {
                let filepath = entry.path();
                if filepath.extension().map(|s| s == "html").unwrap_or(false) {
                    let content = std_fs::read_to_string(&filepath).expect("Failed to read file");
                    posts.push(content);
                }
            }
        }
    }
    HttpResponse::Ok().json(posts) 
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    HttpServer::new(|| {
        App::new()
            .route("/rss", web::get().to(rss_handler))
            .route("/upload", web::post().to(upload_markdown))
            .route("/posts", web::get().to(get_uploaded_posts))
            .route("/add_rss", web::post().to(add_rss_link)) 
            .service(fs::Files::new("/", "templates/").index_file("index.html")) 
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
