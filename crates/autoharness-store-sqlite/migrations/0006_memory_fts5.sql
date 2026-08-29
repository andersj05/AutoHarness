CREATE VIRTUAL TABLE memory_revision_fts USING fts5(
    content,
    revision_id UNINDEXED,
    memory_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);
