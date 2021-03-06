use google_youtube3::YouTube;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{event, instrument, Level};

use crate::{Thumbnail, Thumbnails, Video, THUMBNAILS_DIR, VIDEOS_DIR, WHITESPACE_RE};

// https://developers.google.com/youtube/v3/docs/playlistItems/list
#[instrument(skip(hub))]
async fn get_playlist_items(hub: &YouTube, id: &str) -> Vec<Video> {
    let mut first = true;
    let mut next_page_token: Option<String> = None;
    let parts = vec![
        "snippet".to_string(),
        "contentDetails".to_string(),
        "id".to_string(),
        "status".to_string(),
    ];
    let mut items = Vec::new();

    while first || next_page_token.is_some() {
        first = false;
        let mut call = hub
            .playlist_items()
            .list(&parts)
            .max_results(50)
            .playlist_id(id);

        if let Some(page_token) = &next_page_token {
            call = call.page_token(page_token);
        }

        let (_, items_resp) = call.doit().await.unwrap();

        event!(
            Level::TRACE,
            ?items_resp,
            ?id,
            ?next_page_token,
            "playlist items call"
        );
        next_page_token = items_resp.next_page_token.clone();
        items.extend(
            items_resp
                .items
                .unwrap()
                .iter()
                .filter(|item| is_video_available(item))
                .map(|item| {
                    let content_details = item.content_details.clone().unwrap();
                    let snippet = item.snippet.clone().unwrap();
                    let thumbnails = snippet.thumbnails.unwrap();
                    Video {
                        title: snippet.title.unwrap(),
                        video_published_at: content_details.video_published_at,
                        start_at: content_details.start_at,
                        end_at: content_details.end_at,
                        video_id: content_details.video_id.unwrap(),
                        published_at: snippet.published_at.unwrap(),
                        description: snippet.description.unwrap(),
                        thumbnails: Thumbnails::from(&thumbnails),
                    }
                }),
        )
    }
    event!(
        Level::DEBUG,
        ?items,
        "retrieved playlist items from youtube"
    );
    items
}

fn is_video_available(item: &google_youtube3::api::PlaylistItem) -> bool {
    item.status
        .clone()
        .and_then(|status| {
            status
                .privacy_status
                .map(|privacy_status| privacy_status == "public" || privacy_status == "unlisted")
        })
        .unwrap_or(false)
}

#[instrument(skip(hub))]
async fn update_playlist_items(hub: &YouTube, id: &str) -> Vec<Video> {
    let videos = get_playlist_items(hub, id).await;
    let dir_path = Path::new(VIDEOS_DIR);
    fs::create_dir_all(dir_path).await.unwrap();

    for video in &videos {
        let mut video_file_path = dir_path.to_owned();
        video_file_path.push(format!(
            "{}.json",
            video.video_id,
            //WHITESPACE_RE.replace_all(&video.title, "")
        ));
        event!(Level::DEBUG,
            path = ?video_file_path, ?video,
            "writing video to file",
        );
        let mut video_file = fs::File::create(&video_file_path).await.unwrap();

        video_file
            .write_all(serde_json::to_string_pretty(&video).unwrap().as_bytes())
            .await
            .unwrap();

        download_video_thumbnails(&video).await;
    }
    videos
}

#[instrument]
async fn download_video_thumbnails(video: &Video) {
    let thumbnails_dir = {
        let mut dir = Path::new(THUMBNAILS_DIR).to_owned();
        dir.push(&video.video_id);
        dir
    };
    fs::create_dir_all(&thumbnails_dir).await.unwrap();

    download_thumbnail(&thumbnails_dir, &video.thumbnails.default).await;
    download_thumbnail(&thumbnails_dir, &video.thumbnails.high).await;
    download_thumbnail(&thumbnails_dir, &video.thumbnails.medium).await;
    if let Some(thumbnail) = &video.thumbnails.standard {
        download_thumbnail(&thumbnails_dir, thumbnail).await;
    }
    if let Some(thumbnail) = &video.thumbnails.maxres {
        download_thumbnail(&thumbnails_dir, thumbnail).await;
    }
}

#[instrument]
async fn download_thumbnail(dest_dir: &Path, thumbnail: &Thumbnail) {
    let (_, filename) = thumbnail.url.rsplit_once("/").unwrap();
    let dest_file = {
        let mut file = dest_dir.to_path_buf();
        file.push(filename);
        file
    };

    let image = reqwest::get(&thumbnail.url)
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    let mut thumbnail_file = fs::File::create(&dest_file).await.unwrap();
    thumbnail_file.write_all(&image).await.unwrap();

    event!(Level::DEBUG, ?thumbnail.url, file = ?dest_file, "Downloaded thumbnail");
}

#[instrument(skip(hub))]
pub async fn update_all_playlists_items(hub: &YouTube, playlist_ids: Vec<String>) -> Vec<Video> {
    event!(Level::INFO, ?playlist_ids, "updating playlists");
    let mut videos = Vec::new();

    for id in playlist_ids {
        let playlist_videos = update_playlist_items(hub, &id).await;
        videos.extend(playlist_videos);
    }
    videos
}

#[instrument]
pub async fn read_all_videos() -> Vec<Video> {
    todo!()
}
