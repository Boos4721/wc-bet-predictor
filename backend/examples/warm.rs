// 离线缓存预热:用真实映射函数把 curl 抓取的原始 JSON 转成缓存快照(Vec<Match>)。
// 仅文件 I/O,不联网。用于本会话环境二进制无法出网时手动预热。
use std::fs;
use serde_json::Value;
use wc_bet_predictor::{sporttery, polymarket};

fn main() {
    // 体彩:单文件
    let raw = fs::read_to_string("/tmp/raw_sporttery.json").expect("read sporttery");
    let v: Value = serde_json::from_str(&raw).expect("parse sporttery");
    let ms = sporttery::map_matches(&v);
    fs::write("sporttery_cache.json", serde_json::to_string(&ms).unwrap()).unwrap();
    println!("sporttery: {} rows", ms.len());

    // Polymarket:多页合并成一个数组
    let mut all: Vec<Value> = Vec::new();
    for off in [0, 100, 200, 300] {
        let p = format!("/tmp/raw_poly_{off}.json");
        if let Ok(s) = fs::read_to_string(&p) {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&s) {
                all.extend(arr);
            }
        }
    }
    let pm = polymarket::map_events(&Value::Array(all));
    fs::write("poly_cache.json", serde_json::to_string(&pm).unwrap()).unwrap();
    println!("polymarket: {} rows", pm.len());
}
