pub const K: usize = 12;
pub const MIN_SIMILARITY: f32 = 0.35;
pub const LOOKBACK_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub struct Labeled {
    pub vector: Vec<f32>,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    /// Content the vector was computed from, reused as a few-shot example.
    pub features: String,
    pub corrected: bool,
}

#[derive(Debug, Clone)]
pub struct Neighbor<'a> {
    pub item: &'a Labeled,
    pub similarity: f32,
}

pub fn nearest<'a>(query: &[f32], pool: &'a [Labeled], k: usize) -> Vec<Neighbor<'a>> {
    let mut scored: Vec<Neighbor<'a>> = pool
        .iter()
        .filter(|item| item.vector.len() == query.len())
        .map(|item| Neighbor {
            item,
            similarity: cosine(query, &item.vector),
        })
        .filter(|neighbor| neighbor.similarity >= MIN_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    scored.truncate(k);
    scored
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut norm_a, mut norm_b) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

pub fn encode(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn decode(blob: &[u8]) -> Vec<f32> {
    let (chunks, _) = blob.as_chunks::<4>();
    chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labeled(vector: Vec<f32>, category: &str) -> Labeled {
        Labeled {
            vector,
            category_id: Some(category.to_string()),
            project_id: None,
            features: String::new(),
            corrected: false,
        }
    }

    #[test]
    fn nearest_orders_by_similarity_and_drops_dissimilar_entries() {
        let pool = vec![
            labeled(vec![0.0, 1.0], "far"),
            labeled(vec![1.0, 0.1], "near"),
            labeled(vec![1.0, 0.0], "nearer"),
        ];
        let found = nearest(&[1.0, 0.0], &pool, 5);
        let labels: Vec<&str> = found
            .iter()
            .filter_map(|n| n.item.category_id.as_deref())
            .collect();
        assert_eq!(labels, vec!["nearer", "near"]);
    }

    #[test]
    fn vectors_round_trip_through_blobs() {
        let vector = vec![0.25, -1.5, 3.0];
        assert_eq!(decode(&encode(&vector)), vector);
    }
}
