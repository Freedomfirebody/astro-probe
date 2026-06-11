use crate::{JavaError, Result};
use astro_probe_core::traits::DependencyAnalyzer;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub struct JarAnalyzer;

impl JarAnalyzer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JarAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyAnalyzer for JarAnalyzer {
    type Error = JavaError;

    fn analyze_dependency(
        &self,
        path: &Path,
        local_conn: &Connection,
        workspace_id: &str,
    ) -> std::result::Result<(), Self::Error> {
        let jar_files = if path.is_file() {
            if path.extension().and_then(|s| s.to_str()) == Some("jar") {
                vec![path.to_path_buf()]
            } else {
                vec![]
            }
        } else {
            find_jar_files(path)
        };

        if jar_files.is_empty() {
            return Ok(());
        }

        let global_db_path = get_global_cache_path();
        if let Some(parent) = global_db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let global_conn = Connection::open(&global_db_path)?;
        global_conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;

        init_global_db(&global_conn)?;

        local_conn.execute(
            "CREATE TABLE IF NOT EXISTS local_loaded_jars (
                jar_hash TEXT PRIMARY KEY
            );",
            [],
        )?;

        for jar_path in jar_files {
            if let Ok(data) = std::fs::read(&jar_path) {
                let hash = sha256_hash(&data);

                let is_cached: bool = global_conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM cached_jars WHERE jar_hash = ?1)",
                    [&hash],
                    |row| row.get(0),
                )?;

                if !is_cached {
                    // Try to parse, ignore malformed/empty/corrupted JARs gracefully
                    let _ = parse_jar_file(&jar_path, &hash, &global_conn);
                }

                let already_copied: bool = local_conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM local_loaded_jars WHERE jar_hash = ?1)",
                        [&hash],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                if !already_copied {
                    // Copy-on-Load
                    copy_jar_facts_to_local(local_conn, &hash)?;

                    local_conn.execute(
                        "INSERT OR IGNORE INTO local_loaded_jars (jar_hash) VALUES (?1)",
                        [&hash],
                    )?;
                }

                // Update workspace mapping
                global_conn.execute(
                    "INSERT OR REPLACE INTO workspace_jars (workspace_id, jar_hash) VALUES (?1, ?2)",
                    [workspace_id, &hash],
                )?;
            }
        }
        Ok(())
    }
}

pub fn get_global_cache_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("ASTRO_PROBE_GLOBAL_CACHE_PATH") {
        return PathBuf::from(override_path);
    }
    let mut path = if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data)
    } else {
        std::env::temp_dir()
    };
    path.push("astro-probe");
    path.push("cache");
    path.push("global-cache.db");
    path
}

pub fn init_global_db(conn: &Connection) -> Result<()> {
    conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
    let create_result = (|| -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_jars (
                jar_hash TEXT PRIMARY KEY,
                jar_path TEXT,
                last_accessed INTEGER NOT NULL
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_classes (
                jar_hash TEXT NOT NULL,
                fqn TEXT NOT NULL,
                kind TEXT NOT NULL,
                PRIMARY KEY (jar_hash, fqn)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_class_hierarchy (
                jar_hash TEXT NOT NULL,
                class_fqn TEXT NOT NULL,
                parent_fqn TEXT NOT NULL,
                PRIMARY KEY (jar_hash, class_fqn, parent_fqn)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_method_declarations (
                jar_hash TEXT NOT NULL,
                method_fqn TEXT NOT NULL,
                class_fqn TEXT NOT NULL,
                method_name TEXT NOT NULL,
                params TEXT NOT NULL,
                PRIMARY KEY (jar_hash, method_fqn, params)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_source_assignments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                jar_hash TEXT NOT NULL,
                lhs TEXT NOT NULL,
                rhs TEXT NOT NULL,
                assignment_type TEXT NOT NULL,
                method_fqn TEXT NOT NULL
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_call_sites (
                jar_hash TEXT NOT NULL,
                call_id TEXT NOT NULL,
                method_fqn TEXT NOT NULL,
                receiver TEXT,
                method_name TEXT NOT NULL,
                lhs TEXT,
                static_callee TEXT,
                PRIMARY KEY (jar_hash, call_id)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_call_arguments (
                jar_hash TEXT NOT NULL,
                call_id TEXT NOT NULL,
                arg_index INTEGER NOT NULL,
                arg_var TEXT NOT NULL,
                arg_type TEXT,
                PRIMARY KEY (jar_hash, call_id, arg_index)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_call_edges (
                jar_hash TEXT NOT NULL,
                caller TEXT NOT NULL,
                callee TEXT NOT NULL,
                is_virtual INTEGER NOT NULL,
                PRIMARY KEY (jar_hash, caller, callee)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS workspace_jars (
                workspace_id TEXT NOT NULL,
                jar_hash TEXT NOT NULL,
                PRIMARY KEY (workspace_id, jar_hash)
            );",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cached_method_summaries (
                jar_hash TEXT NOT NULL,
                method_fqn TEXT NOT NULL,
                param_index INTEGER NOT NULL,
                PRIMARY KEY (jar_hash, method_fqn, param_index)
            );",
            [],
        )?;
        Ok(())
    })();
    match create_result {
        Ok(_) => {
            conn.execute("COMMIT;", [])?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK;", []);
            Err(e)
        }
    }
}

pub fn sha256_hash(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut blocks = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    blocks.push(0x80);
    while (blocks.len() + 8) % 64 != 0 {
        blocks.push(0x00);
    }
    blocks.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in blocks.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut result = String::new();
    for &val in &h {
        result.push_str(&format!("{:08x}", val));
    }
    result
}

fn parse_descriptor(desc: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut chars = desc.chars().peekable();

    if chars.next() != Some('(') {
        return params;
    }

    while let Some(&c) = chars.peek() {
        if c == ')' {
            break;
        }
        params.push(parse_next_type(&mut chars));
    }
    params
}

fn parse_next_type<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) -> String {
    let mut array_brackets = String::new();
    while chars.peek() == Some(&'[') {
        chars.next();
        array_brackets.push_str("[]");
    }

    let base_type = match chars.next() {
        Some('B') => "byte".to_string(),
        Some('C') => "char".to_string(),
        Some('D') => "double".to_string(),
        Some('F') => "float".to_string(),
        Some('I') => "int".to_string(),
        Some('J') => "long".to_string(),
        Some('S') => "short".to_string(),
        Some('Z') => "boolean".to_string(),
        Some('L') => {
            let mut name = String::new();
            while let Some(c) = chars.next() {
                if c == ';' {
                    break;
                }
                name.push(c);
            }
            name.replace('/', ".")
        }
        _ => "java.lang.Object".to_string(),
    };

    format!("{}{}", base_type, array_brackets)
}

fn get_opcode_stack_effect(opcode: u8) -> (usize, usize) {
    match opcode {
        0 => (0, 0),
        1..=15 => (0, 1),
        16 | 17 => (0, 1),
        18 | 19 | 20 => (0, 1),
        21..=24 => (0, 1),
        25 => (0, 1),
        26..=45 => (0, 1),
        46..=53 => (2, 1),
        54..=58 => (1, 0),
        59..=78 => (1, 0),
        79..=86 => (3, 0),
        87 => (1, 0),
        88 => (2, 0),
        89 => (1, 2),
        90 => (2, 3),
        91 => (3, 4),
        92 => (2, 4),
        93 => (3, 5),
        94 => (4, 6),
        95 => (2, 2),
        96..=119 => (2, 1),
        120..=125 => (1, 1),
        126..=131 => (2, 1),
        132 => (0, 0),
        133..=147 => (1, 1),
        148..=152 => (2, 1),
        153..=158 => (1, 0),
        159..=166 => (2, 0),
        167 => (0, 0),
        168 => (0, 1),
        169 => (0, 0),
        170 => (1, 0),
        171 => (1, 0),
        172..=176 => (1, 0),
        177 => (0, 0),
        178 => (0, 1),
        179 => (1, 0),
        180 => (1, 1),
        181 => (2, 0),
        182..=186 => (0, 0),
        187 => (0, 1),
        188 | 189 => (1, 1),
        190 => (1, 1),
        191 => (1, 0),
        192 => (1, 1),
        193 => (1, 1),
        194 | 195 => (1, 0),
        196 => (0, 0),
        197 => (0, 0),
        198 | 199 => (1, 0),
        200 | 201 => (0, 0),
        _ => (0, 0),
    }
}

fn get_opcode_len(opcode: u8) -> usize {
    match opcode {
        16 | 18 | 21 | 22 | 23 | 24 | 25 | 54 | 55 | 56 | 57 | 58 | 169 | 188 => 2,
        17 | 19 | 20 | 132 | 153..=168 | 178..=181 | 187 | 189 | 192 | 193 | 198 | 199 => 3,
        197 => 4,
        200 | 201 => 5,
        182 | 183 | 184 => 3,
        185 | 186 => 5,
        _ => 1,
    }
}

fn resolve_method_ref(
    class_file: &cafebabe::ClassFile,
    cp_index: u16,
) -> Option<(String, String, String)> {
    let entry = class_file.constant_pool.get(cp_index as usize)?;
    match &**entry {
        cafebabe::constant_pool::ConstantPoolEntry::MethodRef(
            class_ref_cell,
            name_type_ref_cell,
        )
        | cafebabe::constant_pool::ConstantPoolEntry::InterfaceMethodRef(
            class_ref_cell,
            name_type_ref_cell,
        ) => {
            let class_borrow = class_ref_cell.borrow();
            let name_type_borrow = name_type_ref_cell.borrow();

            // Get class name
            let class_name =
                if let cafebabe::constant_pool::ConstantPoolRef::Resolved(class_entry) =
                    &*class_borrow
                {
                    match &**class_entry {
                        cafebabe::constant_pool::ConstantPoolEntry::ClassInfo(utf8_ref_cell) => {
                            let utf8_borrow = utf8_ref_cell.borrow();
                            if let cafebabe::constant_pool::ConstantPoolRef::Resolved(utf8_entry) =
                                &*utf8_borrow
                            {
                                match &**utf8_entry {
                                    cafebabe::constant_pool::ConstantPoolEntry::Utf8(s) => {
                                        Some(s.replace('/', "."))
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                }?;

            // Get name and type
            let (method_name, method_desc) =
                if let cafebabe::constant_pool::ConstantPoolRef::Resolved(nt_entry) =
                    &*name_type_borrow
                {
                    match &**nt_entry {
                        cafebabe::constant_pool::ConstantPoolEntry::NameAndType(n_cell, d_cell) => {
                            let n_borrow = n_cell.borrow();
                            let d_borrow = d_cell.borrow();
                            if let (
                                cafebabe::constant_pool::ConstantPoolRef::Resolved(ne),
                                cafebabe::constant_pool::ConstantPoolRef::Resolved(de),
                            ) = (&*n_borrow, &*d_borrow)
                            {
                                match (&**ne, &**de) {
                                    (
                                        cafebabe::constant_pool::ConstantPoolEntry::Utf8(n),
                                        cafebabe::constant_pool::ConstantPoolEntry::Utf8(d),
                                    ) => Some((n.to_string(), d.to_string())),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                }?;

            Some((class_name, method_name, method_desc))
        }
        _ => None,
    }
}

fn strip_signature(method_fqn: &str) -> &str {
    if let Some(idx) = method_fqn.find('(') {
        &method_fqn[..idx]
    } else {
        method_fqn
    }
}

pub fn find_jar_files<P: AsRef<Path>>(dir: P) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_jar_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("jar") {
                files.push(path);
            }
        }
    }
    files
}

fn get_load_opcode_slot(opcode: u8, code: &[u8], ip: usize) -> Option<(usize, usize)> {
    match opcode {
        21 | 22 | 23 | 24 | 25 => {
            // iload, lload, fload, dload, aload
            if ip + 1 < code.len() {
                Some((code[ip + 1] as usize, 2))
            } else {
                None
            }
        }
        26..=29 => Some(((opcode - 26) as usize, 1)), // iload_0..3
        30..=33 => Some(((opcode - 30) as usize, 1)), // lload_0..3
        34..=37 => Some(((opcode - 34) as usize, 1)), // fload_0..3
        38..=41 => Some(((opcode - 38) as usize, 1)), // dload_0..3
        42..=45 => Some(((opcode - 42) as usize, 1)), // aload_0..3
        _ => None,
    }
}

fn get_store_opcode_slot(opcode: u8, code: &[u8], ip: usize) -> Option<(usize, usize)> {
    match opcode {
        54 | 55 | 56 | 57 | 58 => {
            // istore, lstore, fstore, dstore, astore
            if ip + 1 < code.len() {
                Some((code[ip + 1] as usize, 2))
            } else {
                None
            }
        }
        59..=62 => Some(((opcode - 59) as usize, 1)), // istore_0..3
        63..=66 => Some(((opcode - 63) as usize, 1)), // lstore_0..3
        67..=70 => Some(((opcode - 67) as usize, 1)), // fstore_0..3
        71..=74 => Some(((opcode - 71) as usize, 1)), // dstore_0..3
        75..=78 => Some(((opcode - 75) as usize, 1)), // astore_0..3
        _ => None,
    }
}

pub fn parse_jar_file(jar_path: &Path, jar_hash: &str, global_conn: &Connection) -> Result<()> {
    let file = File::open(jar_path)?;
    let mut archive = ZipArchive::new(file)?;

    global_conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
    let process_res = (|| -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        global_conn.execute(
            "INSERT OR REPLACE INTO cached_jars (jar_hash, jar_path, last_accessed) VALUES (?1, ?2, ?3)",
            [jar_hash, &jar_path.to_string_lossy(), &now.to_string()],
        )?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.name().ends_with(".class") {
                let mut bytes = Vec::new();
                use std::io::Read;
                if file.read_to_end(&mut bytes).is_err() {
                    continue; // Skip corrupted class/file entry
                }

                if let Ok(class_file) = cafebabe::parse_class(&bytes) {
                    let class_fqn = class_file.this_class.replace('/', ".");
                    let kind = if class_file
                        .access_flags
                        .contains(cafebabe::ClassAccessFlags::INTERFACE)
                    {
                        "interface"
                    } else {
                        "class"
                    };

                    global_conn.execute(
                        "INSERT OR REPLACE INTO cached_classes (jar_hash, fqn, kind) VALUES (?1, ?2, ?3)",
                        [jar_hash, &class_fqn, kind],
                    )?;

                    if let Some(ref super_class) = class_file.super_class {
                        let super_fqn = super_class.replace('/', ".");
                        global_conn.execute(
                            "INSERT OR REPLACE INTO cached_class_hierarchy (jar_hash, class_fqn, parent_fqn) VALUES (?1, ?2, ?3)",
                            [jar_hash, &class_fqn, &super_fqn],
                        )?;
                    }

                    for interface in &class_file.interfaces {
                        let interface_fqn = interface.replace('/', ".");
                        global_conn.execute(
                            "INSERT OR REPLACE INTO cached_class_hierarchy (jar_hash, class_fqn, parent_fqn) VALUES (?1, ?2, ?3)",
                            [jar_hash, &class_fqn, &interface_fqn],
                        )?;
                    }

                    let mut call_site_counter = 0;
                    for method in &class_file.methods {
                        let method_name = &method.name;
                        let desc = &method.descriptor;

                        let param_types = parse_descriptor(desc);
                        let param_names: Vec<String> = (0..param_types.len())
                            .map(|idx| format!("p{}", idx))
                            .collect();
                        let params_str = param_names.join(",");

                        let signature = param_types.join(",");
                        let method_fqn = format!("{}.{}({})", class_fqn, method_name, signature);

                        global_conn.execute(
                            "INSERT OR REPLACE INTO cached_method_declarations (jar_hash, method_fqn, class_fqn, method_name, params) VALUES (?1, ?2, ?3, ?4, ?5)",
                            [jar_hash, &method_fqn, &class_fqn, method_name, &params_str],
                        )?;

                        for (idx, param_name) in param_names.iter().enumerate() {
                            let param_node = format!("{}#{}", method_fqn, param_name);
                            let pos_node = format!("{}#p{}", method_fqn, idx);
                            global_conn.execute(
                                "INSERT OR REPLACE INTO cached_source_assignments (jar_hash, lhs, rhs, assignment_type, method_fqn) VALUES (?1, ?2, ?3, 'COPY', ?4)",
                                [jar_hash, &param_node, &pos_node, &method_fqn],
                            )?;
                        }

                        for attr in &method.attributes {
                            if let cafebabe::attributes::AttributeData::Code(ref code_attr) =
                                attr.data
                            {
                                let mut locals = vec![
                                    None;
                                    (code_attr.max_locals as usize)
                                        .max(param_names.len() * 2 + 2)
                                ];
                                let is_static = method
                                    .access_flags
                                    .contains(cafebabe::MethodAccessFlags::STATIC);
                                let mut current_slot = if is_static { 0 } else { 1 };
                                for (idx, param_type) in param_types.iter().enumerate() {
                                    if current_slot < locals.len() {
                                        locals[current_slot] = Some(idx);
                                    }
                                    if param_type == "long" || param_type == "double" {
                                        current_slot += 2;
                                    } else {
                                        current_slot += 1;
                                    }
                                }

                                let code = &code_attr.code;
                                let mut ip: usize = 0;
                                let mut stack = Vec::new();

                                while ip < code.len() {
                                    let opcode = code[ip];
                                    match opcode {
                                        21..=25 | 26..=45 => {
                                            if let Some((slot, len)) =
                                                get_load_opcode_slot(opcode, code, ip)
                                            {
                                                let val = if slot < locals.len() {
                                                    locals[slot]
                                                } else {
                                                    None
                                                };
                                                stack.push(val);
                                                ip += len;
                                            } else {
                                                ip += 1;
                                            }
                                        }
                                        54..=58 | 59..=78 => {
                                            if let Some((slot, len)) =
                                                get_store_opcode_slot(opcode, code, ip)
                                            {
                                                let val = stack.pop().flatten();
                                                if slot < locals.len() {
                                                    locals[slot] = val;
                                                }
                                                ip += len;
                                            } else {
                                                ip += 1;
                                            }
                                        }
                                        89 => {
                                            // dup
                                            let val =
                                                if let Some(v) = stack.last() { *v } else { None };
                                            stack.push(val);
                                            ip += 1;
                                        }
                                        133..=147 => {
                                            // primitive casts
                                            let val = if !stack.is_empty() {
                                                stack.pop().flatten()
                                            } else {
                                                None
                                            };
                                            stack.push(val);
                                            ip += 1;
                                        }
                                        192 => {
                                            // checkcast
                                            let val = if !stack.is_empty() {
                                                stack.pop().flatten()
                                            } else {
                                                None
                                            };
                                            stack.push(val);
                                            ip += 3;
                                        }
                                        172..=176 => {
                                            // returns with value
                                            if !stack.is_empty() {
                                                if let Some(param_idx) = stack.last().unwrap() {
                                                    global_conn.execute(
                                                        "INSERT OR REPLACE INTO cached_method_summaries (jar_hash, method_fqn, param_index) VALUES (?1, ?2, ?3)",
                                                        [jar_hash, &method_fqn, &param_idx.to_string()],
                                                    )?;
                                                }
                                            }
                                            stack.pop();
                                            ip += 1;
                                        }
                                        182..=185 => {
                                            // invokes
                                            if ip + 2 < code.len() {
                                                let cp_idx = u16::from_be_bytes([
                                                    code[ip + 1],
                                                    code[ip + 2],
                                                ]);
                                                let extra = if opcode == 185 { 2 } else { 0 };
                                                ip += 3 + extra;

                                                if let Some((
                                                    callee_class,
                                                    callee_name,
                                                    callee_desc,
                                                )) = resolve_method_ref(&class_file, cp_idx)
                                                {
                                                    let callee_param_types =
                                                        parse_descriptor(&callee_desc);
                                                    let num_args = callee_param_types.len();

                                                    let mut args = Vec::new();
                                                    for _ in 0..num_args {
                                                        if !stack.is_empty() {
                                                            args.push(stack.pop().unwrap());
                                                        } else {
                                                            args.push(None);
                                                        }
                                                    }
                                                    args.reverse();

                                                    let receiver_val = if opcode != 184 {
                                                        if !stack.is_empty() {
                                                            stack.pop().unwrap()
                                                        } else {
                                                            None
                                                        }
                                                    } else {
                                                        None
                                                    };

                                                    let is_void = callee_desc.ends_with('V');
                                                    if !is_void {
                                                        stack.push(None);
                                                    }

                                                    let call_id = format!(
                                                        "{}:call_{}",
                                                        method_fqn, call_site_counter
                                                    );
                                                    call_site_counter += 1;

                                                    let receiver_node =
                                                        receiver_val.map(|param_idx| {
                                                            let param_name =
                                                                if param_idx < param_names.len() {
                                                                    &param_names[param_idx]
                                                                } else {
                                                                    "p"
                                                                };
                                                            format!("{}#{}", method_fqn, param_name)
                                                        });

                                                    let mut arg_types = Vec::new();
                                                    let mut arg_vars = Vec::new();
                                                    for (arg_idx, arg_val) in
                                                        args.iter().enumerate()
                                                    {
                                                        let arg_type =
                                                            if arg_idx < callee_param_types.len() {
                                                                callee_param_types[arg_idx].clone()
                                                            } else {
                                                                "java.lang.Object".to_string()
                                                            };
                                                        arg_types.push(arg_type.clone());

                                                        let arg_var = match arg_val {
                                                            Some(param_idx) => {
                                                                let param_name = if *param_idx
                                                                    < param_names.len()
                                                                {
                                                                    &param_names[*param_idx]
                                                                } else {
                                                                    "p"
                                                                };
                                                                format!(
                                                                    "{}#{}",
                                                                    method_fqn, param_name
                                                                )
                                                            }
                                                            None => format!(
                                                                "{}:call_arg_{}",
                                                                call_id, arg_idx
                                                            ),
                                                        };
                                                        arg_vars.push(arg_var);
                                                    }

                                                    let static_callee = format!(
                                                        "{}.{}({})",
                                                        callee_class,
                                                        callee_name,
                                                        callee_param_types.join(",")
                                                    );

                                                    global_conn.execute(
                                                        "INSERT OR REPLACE INTO cached_call_sites (jar_hash, call_id, method_fqn, receiver, method_name, lhs, static_callee) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                                                        [
                                                            Some(jar_hash.to_string()),
                                                            Some(call_id.clone()),
                                                            Some(method_fqn.clone()),
                                                            receiver_node,
                                                            Some(callee_name.clone()),
                                                            Some(static_callee.clone()),
                                                        ],
                                                    )?;

                                                    for (arg_idx, (arg_var, arg_type)) in arg_vars
                                                        .iter()
                                                        .zip(arg_types.iter())
                                                        .enumerate()
                                                    {
                                                        global_conn.execute(
                                                            "INSERT OR REPLACE INTO cached_call_arguments (jar_hash, call_id, arg_index, arg_var, arg_type) VALUES (?1, ?2, ?3, ?4, ?5)",
                                                            [
                                                                Some(jar_hash.to_string()),
                                                                Some(call_id.clone()),
                                                                Some(arg_idx.to_string()),
                                                                Some(arg_var.clone()),
                                                                Some(arg_type.clone()),
                                                            ],
                                                        )?;
                                                    }

                                                    let caller_stripped =
                                                        strip_signature(&method_fqn);
                                                    let callee_stripped =
                                                        format!("{}.{}", callee_class, callee_name);
                                                    global_conn.execute(
                                                        "INSERT OR REPLACE INTO cached_call_edges (jar_hash, caller, callee, is_virtual) VALUES (?1, ?2, ?3, 0)",
                                                        [jar_hash, caller_stripped, &callee_stripped],
                                                    )?;
                                                }
                                            } else {
                                                ip += 1;
                                            }
                                        }
                                        196 => {
                                            if ip + 1 < code.len() {
                                                let sub_op = code[ip + 1];
                                                match sub_op {
                                                    25 => {
                                                        if ip + 3 < code.len() {
                                                            let idx = u16::from_be_bytes([
                                                                code[ip + 2],
                                                                code[ip + 3],
                                                            ])
                                                                as usize;
                                                            let val = if idx < locals.len() {
                                                                locals[idx]
                                                            } else {
                                                                None
                                                            };
                                                            stack.push(val);
                                                            ip += 4;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    58 => {
                                                        if ip + 3 < code.len() {
                                                            let idx = u16::from_be_bytes([
                                                                code[ip + 2],
                                                                code[ip + 3],
                                                            ])
                                                                as usize;
                                                            let val = stack.pop().flatten();
                                                            if idx < locals.len() {
                                                                locals[idx] = val;
                                                            }
                                                            ip += 4;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    21 | 22 | 23 | 24 => {
                                                        if ip + 3 < code.len() {
                                                            stack.push(None);
                                                            ip += 4;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    54 | 55 | 56 | 57 => {
                                                        if ip + 3 < code.len() {
                                                            let _ = stack.pop();
                                                            ip += 4;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    169 => {
                                                        if ip + 3 < code.len() {
                                                            ip += 4;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    132 => {
                                                        if ip + 5 < code.len() {
                                                            ip += 6;
                                                        } else {
                                                            ip += 2;
                                                        }
                                                    }
                                                    _ => {
                                                        ip += 2;
                                                    }
                                                }
                                            } else {
                                                ip += 1;
                                            }
                                        }
                                        170 => {
                                            stack.pop();
                                            ip += 1;
                                            while ip % 4 != 0 {
                                                ip += 1;
                                            }
                                            if ip + 12 <= code.len() {
                                                let low = i32::from_be_bytes([
                                                    code[ip + 4],
                                                    code[ip + 5],
                                                    code[ip + 6],
                                                    code[ip + 7],
                                                ]);
                                                let high = i32::from_be_bytes([
                                                    code[ip + 8],
                                                    code[ip + 9],
                                                    code[ip + 10],
                                                    code[ip + 11],
                                                ]);
                                                ip += 12;
                                                if high >= low {
                                                    if let Some(count) = high
                                                        .checked_sub(low)
                                                        .and_then(|diff| diff.checked_add(1))
                                                    {
                                                        let count_usize = count as usize;
                                                        if let Some(offset) =
                                                            count_usize.checked_mul(4)
                                                        {
                                                            if let Some(next_ip) =
                                                                ip.checked_add(offset)
                                                            {
                                                                if next_ip <= code.len() {
                                                                    ip = next_ip;
                                                                } else {
                                                                    ip = code.len();
                                                                }
                                                            } else {
                                                                ip = code.len();
                                                            }
                                                        } else {
                                                            ip = code.len();
                                                        }
                                                    } else {
                                                        ip = code.len();
                                                    }
                                                } else {
                                                    ip = code.len();
                                                }
                                            } else {
                                                ip = code.len();
                                            }
                                        }
                                        171 => {
                                            stack.pop();
                                            ip += 1;
                                            while ip % 4 != 0 {
                                                ip += 1;
                                            }
                                            if ip + 8 <= code.len() {
                                                let npairs = i32::from_be_bytes([
                                                    code[ip + 4],
                                                    code[ip + 5],
                                                    code[ip + 6],
                                                    code[ip + 7],
                                                ]);
                                                ip += 8;
                                                if npairs > 0 {
                                                    let npairs_usize = npairs as usize;
                                                    if let Some(offset) =
                                                        npairs_usize.checked_mul(8)
                                                    {
                                                        if let Some(next_ip) =
                                                            ip.checked_add(offset)
                                                        {
                                                            if next_ip <= code.len() {
                                                                ip = next_ip;
                                                            } else {
                                                                ip = code.len();
                                                            }
                                                        } else {
                                                            ip = code.len();
                                                        }
                                                    } else {
                                                        ip = code.len();
                                                    }
                                                }
                                            } else {
                                                ip = code.len();
                                            }
                                        }
                                        _ => {
                                            let (pop, push) = get_opcode_stack_effect(opcode);
                                            for _ in 0..pop {
                                                if !stack.is_empty() {
                                                    stack.pop();
                                                }
                                            }
                                            for _ in 0..push {
                                                stack.push(None);
                                            }
                                            ip += get_opcode_len(opcode);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })();
    match process_res {
        Ok(_) => {
            global_conn.execute("COMMIT;", [])?;
            Ok(())
        }
        Err(e) => {
            let _ = global_conn.execute("ROLLBACK;", []);
            Err(e)
        }
    }
}

pub fn copy_jar_facts_to_local(conn: &Connection, jar_hash: &str) -> Result<()> {
    let global_db_path = get_global_cache_path();
    let global_db_path_str = global_db_path.to_string_lossy().replace('\\', "/");
    let escaped_path = global_db_path_str.replace("'", "''");

    conn.execute(
        &format!("ATTACH DATABASE '{}' AS global_db", escaped_path),
        [],
    )?;

    conn.execute("BEGIN IMMEDIATE TRANSACTION;", [])?;
    let copy_res = (|| -> Result<()> {
        conn.execute("INSERT OR REPLACE INTO classes (fqn, kind) SELECT fqn, kind FROM global_db.cached_classes WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO class_hierarchy (class_fqn, parent_fqn) SELECT class_fqn, parent_fqn FROM global_db.cached_class_hierarchy WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO method_declarations (method_fqn, class_fqn, method_name, params) SELECT method_fqn, class_fqn, method_name, params FROM global_db.cached_method_declarations WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO source_assignments (lhs, rhs, assignment_type, method_fqn) SELECT lhs, rhs, assignment_type, method_fqn FROM global_db.cached_source_assignments WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO call_sites (call_id, method_fqn, receiver, method_name, lhs, static_callee) SELECT call_id, method_fqn, receiver, method_name, lhs, static_callee FROM global_db.cached_call_sites WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO call_arguments (call_id, arg_index, arg_var, arg_type) SELECT call_id, arg_index, arg_var, arg_type FROM global_db.cached_call_arguments WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO call_edges (caller, callee, is_virtual) SELECT caller, callee, is_virtual FROM global_db.cached_call_edges WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO library_classes (fqn) SELECT fqn FROM global_db.cached_classes WHERE jar_hash = ?1", [jar_hash])?;
        conn.execute("INSERT OR REPLACE INTO method_summaries (method_fqn, param_index) SELECT method_fqn, param_index FROM global_db.cached_method_summaries WHERE jar_hash = ?1", [jar_hash])?;
        Ok(())
    })();

    match copy_res {
        Ok(_) => {
            conn.execute("COMMIT;", [])?;
        }
        Err(ref e) => {
            let _ = conn.execute("ROLLBACK;", []);
        }
    }

    let detach_res = conn.execute("DETACH DATABASE global_db", []);

    copy_res.and(detach_res.map(|_| ()).map_err(|e| e.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_bytecode_parser(code: &[u8]) -> usize {
        let mut ip = 0;
        let mut stack = vec![Some(1)]; // push a dummy stack value
        while ip < code.len() {
            let opcode = code[ip];
            match opcode {
                170 => {
                    stack.pop();
                    ip += 1;
                    while ip % 4 != 0 {
                        ip += 1;
                    }
                    if ip + 12 <= code.len() {
                        let low = i32::from_be_bytes([
                            code[ip + 4],
                            code[ip + 5],
                            code[ip + 6],
                            code[ip + 7],
                        ]);
                        let high = i32::from_be_bytes([
                            code[ip + 8],
                            code[ip + 9],
                            code[ip + 10],
                            code[ip + 11],
                        ]);
                        ip += 12;
                        if high >= low {
                            if let Some(count) =
                                high.checked_sub(low).and_then(|diff| diff.checked_add(1))
                            {
                                let count_usize = count as usize;
                                if let Some(offset) = count_usize.checked_mul(4) {
                                    if let Some(next_ip) = ip.checked_add(offset) {
                                        if next_ip <= code.len() {
                                            ip = next_ip;
                                        } else {
                                            ip = code.len();
                                        }
                                    } else {
                                        ip = code.len();
                                    }
                                } else {
                                    ip = code.len();
                                }
                            } else {
                                ip = code.len();
                            }
                        } else {
                            ip = code.len();
                        }
                    } else {
                        ip = code.len();
                    }
                }
                171 => {
                    stack.pop();
                    ip += 1;
                    while ip % 4 != 0 {
                        ip += 1;
                    }
                    if ip + 8 <= code.len() {
                        let npairs = i32::from_be_bytes([
                            code[ip + 4],
                            code[ip + 5],
                            code[ip + 6],
                            code[ip + 7],
                        ]);
                        ip += 8;
                        if npairs > 0 {
                            let npairs_usize = npairs as usize;
                            if let Some(offset) = npairs_usize.checked_mul(8) {
                                if let Some(next_ip) = ip.checked_add(offset) {
                                    if next_ip <= code.len() {
                                        ip = next_ip;
                                    } else {
                                        ip = code.len();
                                    }
                                } else {
                                    ip = code.len();
                                }
                            } else {
                                ip = code.len();
                            }
                        }
                    } else {
                        ip = code.len();
                    }
                }
                _ => {
                    ip += 1;
                }
            }
        }
        ip
    }

    #[test]
    fn test_tableswitch_valid() {
        let mut code = vec![170, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[0, 0, 0, 1]); // low
        code.extend_from_slice(&[0, 0, 0, 2]); // high
        code.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // jump table

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_tableswitch_malformed_high_less_than_low() {
        let mut code = vec![170, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[0, 0, 0, 10]); // low
        code.extend_from_slice(&[0, 0, 0, 5]); // high
        code.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // jump table

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_tableswitch_overflow_range() {
        let mut code = vec![170, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[128, 0, 0, 0]); // low (i32::MIN)
        code.extend_from_slice(&[127, 255, 255, 255]); // high (i32::MAX)

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_tableswitch_overflow_add_one() {
        let mut code = vec![170, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[0, 0, 0, 0]); // low
        code.extend_from_slice(&[127, 255, 255, 255]); // high (i32::MAX)

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_lookupswitch_valid() {
        let mut code = vec![171, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[0, 0, 0, 2]); // npairs
        code.extend_from_slice(&[0; 16]); // pairs

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_lookupswitch_negative_npairs() {
        let mut code = vec![171, 0, 0, 0];
        code.extend_from_slice(&[0, 0, 0, 0]); // default
        code.extend_from_slice(&[255, 255, 255, 251]); // npairs = -5

        let final_ip = run_bytecode_parser(&code);
        assert_eq!(final_ip, code.len());
    }

    #[test]
    fn test_get_load_and_store_opcode_slots() {
        assert_eq!(get_load_opcode_slot(25, &[25, 5], 0), Some((5, 2)));
        assert_eq!(get_load_opcode_slot(42, &[], 0), Some((0, 1)));
        assert_eq!(get_load_opcode_slot(45, &[], 0), Some((3, 1)));

        assert_eq!(get_store_opcode_slot(58, &[58, 6], 0), Some((6, 2)));
        assert_eq!(get_store_opcode_slot(75, &[], 0), Some((0, 1)));
        assert_eq!(get_store_opcode_slot(78, &[], 0), Some((3, 1)));
    }
}
