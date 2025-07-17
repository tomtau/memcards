use std::sync::Arc;

use crate::models::FlashcardNew;
use anyhow::Result;
use sqlx::PgPool;

fn import_anki_text(
    deck_id: i32,
    front_idx: usize,
    back_idx: usize,
    file: String,
) -> Vec<FlashcardNew> {
    let mut lines = file.lines();
    let mut separator = '\t';
    let sep = "#separator:";
    let mut flashcards = Vec::new();
    while let Some(line) = lines.next() {
        if line.starts_with('#') {
            if line.starts_with(sep) {
                let trimmed = line.trim_start_matches(sep);
                /*
                Comma, Semicolon, Tab, Space, Pipe, Colon, or the according literal characters
                */
                if trimmed.starts_with("tab") {
                    separator = '\t';
                } else if trimmed.starts_with("comma") {
                    separator = ',';
                } else if trimmed.starts_with("semicolon") {
                    separator = ';';
                } else if trimmed.starts_with("space") {
                    separator = ' ';
                } else if trimmed.starts_with("pipe") {
                    separator = '|';
                } else if trimmed.starts_with("colon") {
                    separator = ':';
                } else if trimmed.starts_with("'") {
                    separator = trimmed.chars().nth(1).unwrap_or('\t');
                }
            }
            continue;
        } else if line.trim().is_empty() {
            continue; // Skip empty lines
        } else {
            let parts = line.split(separator);
            let mut front = None;
            let mut back = None;
            for (i, part) in parts.enumerate() {
                if i == front_idx {
                    front = Some(part.trim().to_string());
                } else if i == back_idx {
                    back = Some(part.trim().to_string());
                }
                if i > back_idx && i > front_idx {
                    break;
                }
            }
            if let (Some(front), Some(back)) = (front, back) {
                flashcards.push(FlashcardNew {
                    deck_id,
                    front,
                    back,
                });
            }
        }
    }

    flashcards
}

pub async fn import_anki_text_to_db(
    pool: Arc<PgPool>,
    deck_id: i32,
    front_idx: usize,
    back_idx: usize,
    file: String,
) -> Result<()> {
    let flashcards = import_anki_text(deck_id, front_idx, back_idx, file);
    if flashcards.is_empty() {
        return Ok(());
    } else {
        let mut tx = pool.begin().await?;
        for flashcard in flashcards {
            sqlx::query(
                "INSERT INTO flashcard (deck_id, front, back) VALUES ($1, $2, $3)",
                
            )
            .bind(flashcard.deck_id)
            .bind(flashcard.front)
            .bind(flashcard.back)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    #[test]
    fn test_importer() {
        let sample = r#"#separator:tab
#html:false
.	Come on!	[Cc]ome +on[ \!]?		嚟 啦 。	嚟 啦 。	le̖i lā.	lei4 laa1.	leih4 la1.	lei4 laa1.	((嚟|[o口]黎|來)|[Ll]ei4?|[Ll]eih4?)\W*(啦|[Ll]aa1?|[Ll]a1?)\W*			come !	lei4 laa1. / leih4 la1.	le̖ʲ lāː 	1
.	He dances.	[Hh]e +dances[ \.]?		佢 跳舞 。	佢 跳舞 。	kö̗ü tiu̟ mo̗u.	keoi5 tiu3 mou5.	keuih5 tiu3 mouh5.	keoi5 tiu3 mou5.	((佢|人巨|他)|[Kk]eoi5?|[Kk]euih5?)\W*(跳|[Tt]iu3?)\W*(舞|[Mm]ou5?|[Mm]ouh5?)\W*			s/he dance	keoi5 tiu3 mou5. / keuih5 tiu3 mouh5.	kʰø̗ᶣ tʰi̟ːʷ mo̗ʷ 	2"#;

        let cards = super::import_anki_text(1, 1, 7, sample.to_string());
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].front, "Come on!");
        assert_eq!(cards[0].back, "lei4 laa1.");
        assert_eq!(cards[1].front, "He dances.");
        assert_eq!(cards[1].back, "keoi5 tiu3 mou5.");
    }
}
