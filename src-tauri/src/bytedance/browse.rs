use super::{
    client::{MarketClient, API_BASE},
    models::{Board, Page},
    search_builder::SearchBuilder,
};
use crate::core::{error::AppError, skill_store::SkillStore};

pub async fn fetch(
    store: &SkillStore,
    board: Board,
    query: &str,
    page: u32,
    page_size: u32,
    refresh: bool,
) -> Result<Page, AppError> {
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);
    let key = format!(
        "bytedance:v1:{}",
        serde_json::to_string(&(board, query.trim(), page, page_size))
            .map_err(AppError::internal)?
    );
    if !refresh {
        let cached = store.get_cache(&key, 300).map_err(AppError::db)?;
        if cached.is_some() {
            let text = cached.unwrap();
            match serde_json::from_str::<Page>(&text) {
                Ok(result) => return Ok(result),
                Err(_) => {
                    // 缓存解析失败，继续请求接口。
                }
            }
        }
    }

    // 后面继续请求接口
    let url = SearchBuilder::new(API_BASE)
        .board(board)
        .query(query)
        .page(page)
        .page_size(page_size)
        .build()?;
    let proxy = store.proxy_url();
    let client = MarketClient::shared(proxy.as_deref())?;
    let result = client.send(url, board).await?;
    store
        .set_cache(
            &key,
            &serde_json::to_string(&result).map_err(AppError::internal)?,
        )
        .map_err(AppError::db)?;
    Ok(result)
}
