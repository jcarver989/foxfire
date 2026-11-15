use clankerdiff_core::{DiffDocument, DiffSide, testing::DocumentBuilder};
use clankerdiff_symbols::DocumentSyntax;
use std::{sync::Arc, thread};

const SOURCE: &str = "/// docs\nfn a() -> u32 { let x = \"s\"; x.len() as u32 }\nstruct S { f: Vec<u8> }\n";

// Regression: macOS's system allocator aborted with "pointer being freed was not allocated"
// when a review followed a new document while short-lived threads prepared the old one.
// The race only fires in optimized builds (`cargo test --release`).
#[test]
fn a_review_follows_new_documents_while_other_threads_prepare_its_syntax() {
    let mut syntax = Arc::new(DocumentSyntax::new(document(0)));
    for revision in 1..400 {
        let workers: Vec<_> = (0..9)
            .map(|file| {
                let syntax = Arc::clone(&syntax);
                thread::spawn(move || {
                    syntax.prepare(file);
                    drop(syntax.symbols());
                })
            })
            .collect();
        syntax = Arc::new(syntax.followed(&document(revision)));
        for worker in workers {
            worker.join().expect("preparing syntax does not panic");
        }
    }
    assert!(syntax.document().files[0].source_document(DiffSide::New).is_some());
}

fn document(revision: usize) -> Arc<DiffDocument> {
    let edited = if revision.is_multiple_of(3) { format!("{SOURCE}// r{revision}\n") } else { SOURCE.to_owned() };
    DocumentBuilder::new().changed("src/lib.rs", SOURCE, &edited).build()
}
