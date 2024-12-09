local ffi = require("ffi")

ffi.cdef([[
typedef struct {
    bool error;
} SaveResult;

typedef struct TermFrequencies* TermFrequenciesHandle;
typedef struct TfIdf* TfIdfHandle;

// SaveResult save_json(void);
// SaveResult save_input_number_as_json(int);
// SaveResult save_input_number_as_json_to_custom_path(int, const char*);

TermFrequenciesHandle tf_create();
void tf_insert_term(TermFrequenciesHandle, const char*, float);

TfIdfHandle tf_idf_create();
void tf_idf_insert_doc_tfs(TfIdfHandle, const char*, TermFrequenciesHandle);
void tf_idf_finish(TfIdfHandle);
void tf_idf_save(TfIdfHandle, const char*);

// void save_tf_idf(TfIdfHandle, const char*);


]])

local lib = ffi.load("/Users/matth/projects/rfsee/crates/ffi/target/debug/libffi.dylib")

return lib
