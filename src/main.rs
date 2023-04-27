extern crate clap;
use clap::{App, Arg};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::SeekFrom;
use std::ops::Bound::Included;
use std::path::Path;
use std::process;
use std::vec::Vec;

#[derive(Debug)]
struct Line {
    address: u64,
    size: u64,
    line_number: i64,
    source_file_id: i64,
}

#[derive(Debug, Default)]
struct Function {
    address: u64,
    size: u64,
    stack_param_size: i64,
    name: String,
    is_multiple: bool,
}

#[derive(Debug)]
struct PublicSymbol {
    address: u64,
    stack_param_size: i64,
    name: String,
    is_multiple: bool,
}

#[derive(Debug)]
struct Symbol {
    function_name: String,
    source_file_name: String,
    source_file_number: i64,
}

#[derive(Debug)]
struct SymbolFile {
    files: HashMap<i64, String>,
    functions: RangeMap<Function>,
    lines: RangeMap<Line>,
    public_symbols: BTreeMap<u64, PublicSymbol>,
}

#[derive(Debug)]
struct RangeItem<T> {
    item: T,
    size: u64,
}

#[derive(Debug)]
struct RangeMap<T> {
    map: BTreeMap<u64, RangeItem<T>>,
}

impl<T> RangeMap<T> {
    pub fn new() -> Self {
        RangeMap {
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, address: u64, size: u64, item: T) {
        self.map.insert(address, RangeItem { item, size });
    }

    pub fn retrieve_range<'a>(&'a self, address: u64) -> Option<(&'a T, u64)> {
        if let Some(greater_one) = self
            .map
            .range((Included(&0), Included(&address)))
            .next_back()
        {
            let target_address = *greater_one.0;
            let range_item = greater_one.1;
            let target_size = range_item.size;
            //println!("Found item {:#x},", target_address);
            if target_address <= address && address <= target_address + target_size {
                return Some((&range_item.item, address - target_address));
            }
        }
        None
    }
}

fn find_public_symbol_by_address<'a>(
    map: &'a BTreeMap<u64, PublicSymbol>,
    address: u64,
) -> Option<(&'a PublicSymbol, u64)> {
    if let Some(greater_one) = map.range((Included(&0), Included(&address))).next_back() {
        let target_address = *greater_one.0;
        let target_item = greater_one.1;
        //println!("Found address {:#x}", target_address);
        if target_address <= address {
            return Some((&target_item, address - target_address));
        }
    }
    None
}

fn lookup_address(symbol_file: &SymbolFile, address: u64) -> Option<(Symbol, u64)> {
    if let Some((function_record, offset)) = symbol_file.functions.retrieve_range(address) {
        let mut symbol = Symbol {
            function_name: function_record.name.clone(),
            source_file_name: String::from(""),
            source_file_number: -1,
        };
        let mut offset_from_symbol: u64 = 0;

        if let Some((line, offset)) = symbol_file.lines.retrieve_range(address) {
            offset_from_symbol = offset;
            symbol.source_file_number = line.line_number as i64;
            if let Some(filename) = symbol_file.files.get(&line.source_file_id) {
                symbol.source_file_name = filename.to_string();
            }
        }
        Some((symbol, offset_from_symbol))
    } else if let Some((public_record, offset)) =
        find_public_symbol_by_address(&symbol_file.public_symbols, address)
    {
        let symbol = Symbol {
            function_name: public_record.name.clone(),
            source_file_name: String::from(""),
            source_file_number: -1,
        };
        Some((symbol, offset))
    } else {
        None
    }
}

fn fast_search(reader: &mut BufReader<File>, address: u64) -> Option<Symbol> {
    let symbol: Option<Symbol> = fast_search_line_by_line(reader, address, "FUNC ");
    if symbol.is_some() {
        // TODO: find line info
        return symbol;
    }

    let symbol: Option<Symbol> = fast_search_line_by_line(reader, address, "PUBLIC ");
    if symbol.is_some() {
        return symbol;
    }

    None
}

fn fast_search_line_by_line(reader: &mut BufReader<File>, address: u64, prefix: &str) -> Option<Symbol> {
    reader.seek(SeekFrom::Start(0)).unwrap(); // reset

    //let mut addresses_index = Vec::new();
    for line in reader.lines() {
        let line = line.unwrap(); // Ignore errors.
        let line = line.trim();
        if line.starts_with(prefix) {
            assert_eq!(prefix == "FUNC " || prefix == "PUBLIC ", true);
            //println!("{:?}", line);
            let tmp: Vec<&str> = line.split(" ").collect();
            // TODO: check index in-bounds
           
            let start_addr: u64;
            let mut offset = 0;
            if tmp[1] == "m" {
                offset = 1;
            } 
            start_addr = u64::from_str_radix(tmp[offset + 1], 16).unwrap();

            if prefix == "FUNC " {
                if start_addr <= address { // && address <= end_addr
                    let size: u64 = u64::from_str_radix(tmp[offset + 2], 16).unwrap();
                    let end_addr: u64 = start_addr + size;
                    if address <= end_addr {
                        let name = tmp[offset + 4];
                        return Some(Symbol {
                            function_name: String::from(name),
                            source_file_name: String::from(""),
                            source_file_number: -1,
                        })
                    }
                }
            } else if prefix == "PUBLIC " {
                if start_addr >= address { 
                    let name = tmp[offset + 3];
                    return Some(Symbol {
                        function_name: String::from(name),
                        source_file_name: String::from(""),
                        source_file_number: -1,
                    })
                }
            }
        }
    }

    None
}


fn parse_breakpad_symbol_file(filename: &Path) -> SymbolFile {
    let file = File::open(filename).unwrap();
    let reader = BufReader::new(file);

    let mut symbol_file = SymbolFile {
        files: HashMap::new(),
        functions: RangeMap::new(),
        lines: RangeMap::new(),
        public_symbols: BTreeMap::new(),
    };

    for line in reader.lines() {
        let line = line.unwrap(); // Ignore errors.
        //println!("{:?}", line);
        if line.starts_with("FILE ") {
            parse_file_line(&mut symbol_file, &line);
        } else if line.starts_with("STACK ") {
            // pass
        } else if line.starts_with("FUNC ") {
            parse_func_line(&mut symbol_file, &line);
        } else if line.starts_with("PUBLIC ") {
            parse_public_line(&mut symbol_file, &line);
        } else if line.starts_with("MODULE ") {
            // MODULE <guid> <age> <filename>
            // pass
        } else if line.starts_with("INFO ") {
            // INFO CODE_ID <code id> <filename>
            // pass
        } else {
            // LINE
            parse_line_line(&mut symbol_file, &line);
        }
    }

    //println!("{:?}", symbol_file);
    symbol_file
}

fn parse_line_line(symbol: &mut SymbolFile, line: &str) {
    // <address> <size> <line number> <source file id>
    let line = line.trim();

    let tokens: Vec<&str> = tokenize(line, " ", 4);
    let address = tokens.get(0).unwrap();
    let size = tokens.get(1).unwrap();
    let line_number = tokens.get(2).unwrap();
    let source_file_id = tokens.get(3).unwrap();

    //println!("address={:?}, size={:?}, line_number={:?} source_file_id={:?}", address, size, line_number, source_file_id);
    let address: u64 = u64::from_str_radix(address, 16).unwrap();
    let size: u64 = u64::from_str_radix(size, 16).unwrap();
    let line_number: i64 = i64::from_str_radix(line_number, 10).unwrap();
    let source_file_id: i64 = i64::from_str_radix(source_file_id, 10).unwrap();

    let line = Line {
        address,
        size,
        line_number,
        source_file_id,
    };
    symbol.lines.insert(address, size, line);
}

fn parse_public_line(symbol: &mut SymbolFile, line: &str) {
    // PUBLIC [<multiple>] <address> <stack_param_size> <name>
    assert_eq!(line.starts_with("PUBLIC "), true);
    let line = &line[7..]; // skip prefix
    let line = line.trim();

    let tokens: Vec<&str> = tokenize_with_optional_field(line, "m", " ", 4);
    let is_multiple = tokens.len() >= 4 && *tokens.get(0).unwrap() == "m";
    let mut offset = 0;
    if is_multiple {
        offset = 1;
    }
    let address = tokens.get(offset + 0).unwrap();
    let stack_param_size = tokens.get(offset + 1).unwrap();
    let name = tokens.get(offset + 2).unwrap();

    //println!("name={:?} address={:?}, stack_param_size={:?}", name, address, stack_param_size);
    let address: u64 = u64::from_str_radix(address, 16).unwrap();
    let stack_param_size: i64 = i64::from_str_radix(stack_param_size, 16).unwrap();

    let public_symbol = PublicSymbol {
        address,
        stack_param_size,
        name: String::from(*name),
        is_multiple,
    };
    symbol.public_symbols.insert(address, public_symbol);
}

fn parse_func_line(symbol: &mut SymbolFile, line: &str) {
    // FUNC [<multiple>] <address> <size> <stack_param_size> <name>
    assert_eq!(line.starts_with("FUNC "), true);
    let line = &line[5..]; // skip prefix
    let line = line.trim();

    let tokens: Vec<&str> = tokenize_with_optional_field(line, "m", " ", 5);
    let is_multiple = tokens.len() >= 5 && *tokens.get(0).unwrap() == "m";
    let mut offset = 0;
    if is_multiple {
        offset = 1;
    }
    let address = tokens.get(offset + 0).unwrap();
    let size = tokens.get(offset + 1).unwrap();
    let stack_param_size = tokens.get(offset + 2).unwrap();
    let name = tokens.get(offset + 3).unwrap();

    //println!("address={:?}, size={:?}", address, size);
    let address: u64 = u64::from_str_radix(address, 16).unwrap();
    let size: u64 = u64::from_str_radix(size, 16).unwrap();
    let stack_param_size: i64 = i64::from_str_radix(stack_param_size, 16).unwrap();

    let function = Function {
        address,
        size,
        name: String::from(*name),
        is_multiple,
        stack_param_size,
    };
    symbol.functions.insert(address, size, function);
}

fn parse_file_line(symbol: &mut SymbolFile, line: &str) {
    // FILE <id> <filename>
    assert_eq!(line.starts_with("FILE "), true);
    let line = &line[5..]; // skip prefix
    let line = line.trim();

    let tokens: Vec<&str> = tokenize(line, " ", 2);
    let id = tokens.get(0).unwrap();
    let filename = tokens.get(1).unwrap();
    let id: i64 = i64::from_str_radix(id, 10).unwrap();
    //println!("id={}, filename={}", id, filename);
    symbol.files.insert(id, String::from(*filename));
}

fn tokenize_with_optional_field<'a>(line: &'a str, optional_field: &str, token: &str, max_tokens: usize) -> Vec<&'a str> {
    // First tokenize assuming the optional field is not present.  If we then see
    // the optional field, additionally tokenize the last token into two tokens
    let mut tokens = tokenize(line, token, max_tokens - 1);

    let first = *tokens.get(0).unwrap_or(&"");
    if first == optional_field {
        let last = *tokens.get(tokens.len() - 1).unwrap_or(&"");
        let sub_tokens = tokenize(last, token, 2);
        tokens.remove(tokens.len() - 1);
        return [tokens, sub_tokens].concat();
    }

    tokens
}

fn tokenize<'a>(line: &'a str, token: &str, max_tokens: usize) -> Vec<&'a str> {
    let mut result = Vec::new();
    let mut remaining = max_tokens - 1;
    let mut txt = line;

    let mut tmp = txt.splitn(2, token);
    let mut part_a = tmp.next().unwrap_or("");
    txt = tmp.next().unwrap_or("");
    //println!("tokenize txt={}, token={}, max_tokens={}", line, token, max_tokens);
    while part_a != "" && remaining > 0 {
        result.push(part_a);
        //println!("remaining={}, part_a={}, part_b={}", remaining, part_a, txt);
        if remaining > 1 {
            tmp = txt.splitn(2, token);
            part_a = tmp.next().unwrap_or("");
            txt = tmp.next().unwrap_or("");
        }
        remaining -= 1;
    }

    if remaining == 0 && txt.len() > 0 {
        //println!("remaining={}, part_a={}, part_b={}", remaining, txt, "");
        result.push(txt);
    }

    result
}

fn parse_address(address: &str) -> Option<i64> {
    let addr;
    let mut radio: i64 = 1;
    if address.starts_with("0x") {
        addr = &address[2..];
    } else if address.starts_with("-0x") {
        addr = &address[3..];
        radio = -1;
    } else {
        addr = address;
    }

    return match i64::from_str_radix(addr, 16) {
        Ok(result) => Some(result * radio),
        Err(_) => None
    };
}

// https://chromium.googlesource.com/breakpad/breakpad/+/master/docs/symbol_files.md
fn main() {
    let matches = App::new("addr2line for Breakpad symbol file")
        .version("1.0")
        .author("liudingsan <lds2012@gmail.com>")
        .arg(Arg::with_name("input").help("input symbol file"))
        .arg(Arg::with_name("address").help("address to lookup").multiple(true))
        .arg(Arg::with_name("offset").short("o").long("offset").help("offset of addresses").takes_value(true))
        .arg(Arg::with_name("noline").long("noline").help("skip source file name and line number").takes_value(false))
        .get_matches();

    let input = matches.value_of("input").unwrap();
    let input = Path::new(input);
    if !input.exists() {
        println!("input file({}) is not exists", input.display());
        process::exit(-1);
    }

    let offset = parse_address(matches.value_of("offset").unwrap_or("0")).unwrap_or(0);
    //println!("offset: {} -> {}", matches.value_of("offset").unwrap_or("0"), offset);
    let addresses: Vec<u64> = matches.values_of("address").unwrap().map(|addr| {
        let addr = parse_address(addr).unwrap();
        let offset_addr: i64 = addr + offset as i64;
        return if (offset_addr > 0) { offset_addr as u64 } else { 0 };
    }).collect();

    if let Some(_) = matches.values_of("noline") {
        let file = File::open(input).unwrap();
        let mut reader = BufReader::new(file);

        for address in addresses {
            // Fast search 
            if let Some(symbol) = fast_search(&mut reader, address) {
                let source_file_name = if symbol.source_file_name.len() != 0 { symbol.source_file_name } else { String::from("??") };
                let source_file_number = if symbol.source_file_number != -1 { symbol.source_file_number.to_string() } else { String::from("?") };
                println!(
                    "{:#x} {} {}:{}",
                    address, symbol.function_name, source_file_name, source_file_number
                );
            } else {
                println!("Not found symbol for address {:#x}", address);
            }
        }
    } else {
         // Parse all symbols first
         let symbol_file = parse_breakpad_symbol_file(input);
         for address in addresses {
             if let Some((symbol, offset)) = lookup_address(&symbol_file, address) {
                 let source_file_name = if symbol.source_file_name.len() != 0 { symbol.source_file_name } else { String::from("??") };
                 let source_file_number = if symbol.source_file_number != -1 { symbol.source_file_number.to_string() } else { String::from("?") };
                 println!(
                     "{:#x} {}+{} {}:{}",
                     address, symbol.function_name, offset, source_file_name, source_file_number
                 );
             } else {
                 println!("Not found symbol for address({:#x}", address);
             }
         }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        println!("test_tokenize");

        let tokens = tokenize("c1d11c 0 bool UnityDefaultAllocator<LowLevelAllocator>::AllocationPage<(RequestType)0>(void const*) const", " ", 3);
        //for token in tokens.iter() {
        //    println!("token: {}", token);
        //}
        assert_eq!(tokens.len(), 3);
        assert_eq!(*tokens.get(0).unwrap(), "c1d11c");
        assert_eq!(*tokens.get(1).unwrap(), "0");
        assert_eq!(*tokens.get(2).unwrap(), "bool UnityDefaultAllocator<LowLevelAllocator>::AllocationPage<(RequestType)0>(void const*) const");

        let tokens = tokenize_with_optional_field("m c1d11c 0 bool UnityDefaultAllocator<LowLevelAllocator>::AllocationPage<(RequestType)0>(void const*) const", "m", " ", 4);
        assert_eq!(tokens.len(), 4);
        assert_eq!(*tokens.get(0).unwrap(), "m");
        assert_eq!(*tokens.get(1).unwrap(), "c1d11c");
        assert_eq!(*tokens.get(2).unwrap(), "0");
        assert_eq!(*tokens.get(3).unwrap(), "bool UnityDefaultAllocator<LowLevelAllocator>::AllocationPage<(RequestType)0>(void const*) const");
    }

    #[test]
    fn test_find_function_by_address() {
        println!("test_find_function_by_address");
        let mut symbol_file = SymbolFile {
            files: HashMap::new(),
            functions: RangeMap::new(),
            public_symbols: BTreeMap::new(),
            lines: RangeMap::new(),
        };

        symbol_file.functions.insert(
            0,
            2,
            Function {
                address: 0,
                size: 2,
                ..Default::default()
            },
        ); // 0-2
        symbol_file.functions.insert(
            2,
            1,
            Function {
                address: 2,
                size: 1,
                ..Default::default()
            },
        ); // 2-3
        symbol_file.functions.insert(
            3,
            1,
            Function {
                address: 3,
                size: 1,
                ..Default::default()
            },
        ); // 3-4
        symbol_file.functions.insert(
            6,
            1,
            Function {
                address: 6,
                size: 1,
                ..Default::default()
            },
        ); // 6-7
        symbol_file.functions.insert(
            7,
            1,
            Function {
                address: 7,
                size: 1,
                ..Default::default()
            },
        ); // 7-8

        let address = 3;
        let result = symbol_file.functions.retrieve_range(address);
        assert_eq!(result.is_none(), false);
        let (target, offset) = result.unwrap();
        assert_eq!(target.address, 3);

        let address = 5;
        let result = symbol_file.functions.retrieve_range(address);
        assert_eq!(result.is_none(), true);
    }
}
