use super::{ClassifiedStatement, ParsedStatement, MAX_STATEMENTS};
use crate::guard::QueryRisk;
use crate::types::{DbCell, DbError, DbQueryRequest};
use base64::Engine;
use std::collections::BTreeSet;

pub(super) fn parse(request: &DbQueryRequest) -> Result<Vec<ClassifiedStatement>, DbError> {
    let lines = tokenize(&request.text)?;
    let mut result = Vec::with_capacity(lines.len());
    let mut used = BTreeSet::new();
    for mut argv in lines {
        for (index, argument) in argv.iter_mut().enumerate() {
            if argument.starts_with(b"{{") && argument.ends_with(b"}}") {
                if index == 0 {
                    return Err(DbError::InvalidRequest);
                }
                let name = std::str::from_utf8(&argument[2..argument.len() - 2])
                    .map_err(|_| DbError::InvalidRequest)?;
                let parameter = request
                    .parameters
                    .iter()
                    .find(|value| value.name == name)
                    .ok_or(DbError::InvalidRequest)?;
                used.insert(parameter.name.clone());
                *argument = match &parameter.value {
                    DbCell::Null => return Err(DbError::InvalidRequest),
                    DbCell::Bool(value) => {
                        if *value {
                            b"1".to_vec()
                        } else {
                            b"0".to_vec()
                        }
                    }
                    DbCell::Binary(value) => base64::engine::general_purpose::STANDARD
                        .decode(value)
                        .map_err(|_| DbError::InvalidRequest)?,
                    DbCell::Int(value)
                    | DbCell::Uint(value)
                    | DbCell::Decimal(value)
                    | DbCell::Float(value)
                    | DbCell::Text(value)
                    | DbCell::Date(value)
                    | DbCell::Time(value)
                    | DbCell::Timestamp(value)
                    | DbCell::Json(value) => value.as_bytes().to_vec(),
                };
            } else if argument
                .windows(2)
                .any(|part| part == b"{{" || part == b"}}")
            {
                return Err(DbError::InvalidRequest);
            }
        }
        let (risk, reason) = classify(&argv)?;
        result.push(ClassifiedStatement {
            parsed: ParsedStatement::Redis { argv },
            risk,
            reason,
        });
    }
    if used.len() != request.parameters.len() {
        return Err(DbError::InvalidRequest);
    }
    Ok(result)
}

fn classify(argv: &[Vec<u8>]) -> Result<(QueryRisk, &'static str), DbError> {
    let command = word(argv.first().ok_or(DbError::InvalidRequest)?)?;
    let count = argv.len() - 1;
    let exact = |wanted| {
        if count == wanted {
            Ok(())
        } else {
            Err(DbError::InvalidRequest)
        }
    };
    match command.as_str() {
        "GET" | "GETRANGE" | "STRLEN" | "TYPE" | "TTL" | "PTTL" | "EXPIRETIME" | "PEXPIRETIME"
        | "HLEN" | "HKEYS" | "HVALS" | "HGETALL" | "LLEN" | "SCARD" | "SMEMBERS" | "ZCARD"
        | "XLEN" | "DUMP" => {
            exact(if command == "GETRANGE" { 3 } else { 1 })?;
        }
        "HGET" | "HEXISTS" | "HSTRLEN" | "LINDEX" | "SISMEMBER" | "ZSCORE" | "GETBIT" => exact(2)?,
        "LRANGE" | "ZCOUNT" | "ZLEXCOUNT" => exact(3)?,
        "MGET" | "EXISTS" | "TOUCH" | "PFCOUNT" | "SUNION" | "SINTER" | "SDIFF" => {
            if count == 0 {
                return Err(DbError::InvalidRequest);
            }
        }
        "HMGET" | "SMISMEMBER" | "ZMSCORE" | "GEOHASH" | "GEOPOS" => {
            if count < 2 {
                return Err(DbError::InvalidRequest);
            }
        }
        "ZINTER" | "ZUNION" | "ZDIFF" | "ZINTERCARD" | "SINTERCARD" => {
            if count < 2 {
                return Err(DbError::InvalidRequest);
            }
            bounded_integer(&argv[1], 1, 1000)?;
        }
        "BITCOUNT" | "BITPOS" | "HRANDFIELD" | "SRANDMEMBER" | "ZRANDMEMBER" | "ZRANK"
        | "ZREVRANK" | "LPOS" | "GEOSEARCH" | "GEODIST" | "SORT_RO" => {
            if count == 0 {
                return Err(DbError::InvalidRequest);
            }
        }
        "ZRANGE" | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZRANGEBYLEX"
        | "ZREVRANGEBYLEX" | "XRANGE" | "XREVRANGE" => {
            if count < 3 {
                return Err(DbError::InvalidRequest);
            }
        }
        "SCAN" | "HSCAN" | "SSCAN" | "ZSCAN" => {
            let prefix = if command == "SCAN" { 1 } else { 2 };
            if count < prefix {
                return Err(DbError::InvalidRequest);
            }
            bounded_integer(&argv[prefix], 0, u64::MAX)?;
            let mut offset = prefix + 1;
            while offset < argv.len() {
                let option = word(&argv[offset])?;
                if option == "NOVALUES" && command == "HSCAN" {
                    offset += 1;
                    continue;
                }
                if !matches!(option.as_str(), "MATCH" | "COUNT" | "TYPE")
                    || (option == "TYPE" && command != "SCAN")
                    || offset + 1 == argv.len()
                {
                    return Err(DbError::InvalidRequest);
                }
                if option == "COUNT" {
                    bounded_integer(&argv[offset + 1], 1, 1000)?;
                }
                offset += 2;
            }
        }
        "XREAD" => {
            if count < 3
                || argv
                    .iter()
                    .skip(1)
                    .any(|arg| arg.eq_ignore_ascii_case(b"BLOCK"))
            {
                return Err(DbError::PolicyDenied);
            }
        }
        "BITFIELD_RO" => {
            if count < 4 || (count - 1) % 3 != 0 {
                return Err(DbError::InvalidRequest);
            }
            for operation in argv[2..].chunks_exact(3) {
                if !operation[0].eq_ignore_ascii_case(b"GET") {
                    return Err(DbError::PolicyDenied);
                }
            }
        }
        "OBJECT" => {
            exact(2)?;
            if !matches!(
                word(&argv[1])?.as_str(),
                "ENCODING" | "FREQ" | "IDLETIME" | "REFCOUNT"
            ) {
                return Err(DbError::PolicyDenied);
            }
        }
        "MEMORY" => {
            if !matches!(count, 2 | 4) || !argv[1].eq_ignore_ascii_case(b"USAGE") {
                return Err(DbError::PolicyDenied);
            }
            if count == 4 {
                if !argv[3].eq_ignore_ascii_case(b"SAMPLES") {
                    return Err(DbError::InvalidRequest);
                }
                bounded_integer(&argv[4], 1, 1000)?;
            }
        }
        "XINFO" => {
            if count < 2 {
                return Err(DbError::InvalidRequest);
            }
            match word(&argv[1])?.as_str() {
                "GROUPS" => exact(2)?,
                "CONSUMERS" => exact(3)?,
                "STREAM" => {
                    if !matches!(count, 2 | 3 | 5) {
                        return Err(DbError::InvalidRequest);
                    }
                    if count >= 3 && !argv[3].eq_ignore_ascii_case(b"FULL") {
                        return Err(DbError::InvalidRequest);
                    }
                    if count == 5 {
                        if !argv[4].eq_ignore_ascii_case(b"COUNT") {
                            return Err(DbError::InvalidRequest);
                        }
                        bounded_integer(&argv[5], 1, 1000)?;
                    }
                }
                _ => return Err(DbError::PolicyDenied),
            }
        }
        "INFO" => {
            if count > 1 {
                return Err(DbError::InvalidRequest);
            }
        }
        "DBSIZE" | "TIME" | "LASTSAVE" => exact(0)?,
        "PING" => {
            if count > 1 {
                return Err(DbError::InvalidRequest);
            }
        }
        "ECHO" => exact(1)?,
        // These alter the authorization, selected database or wire protocol,
        // bypass guard visibility, or create an unbounded subscription.
        "AUTH" | "HELLO" | "SELECT" | "RESET" | "QUIT" | "CLIENT" | "ACL" | "CONFIG" | "MULTI"
        | "EXEC" | "DISCARD" | "WATCH" | "UNWATCH" | "READONLY" | "READWRITE" | "ASKING"
        | "CLUSTER" | "REPLICAOF" | "SLAVEOF" | "MODULE" | "DEBUG" | "SHUTDOWN" | "MIGRATE"
        | "REPLCONF" | "SUBSCRIBE" | "PSUBSCRIBE" | "SSUBSCRIBE" | "SYNC" | "PSYNC" | "KEYS"
        | "MONITOR" => return Err(DbError::PolicyDenied),
        "FLUSHALL" | "FLUSHDB" | "SWAPDB" | "EVAL" | "EVALSHA" | "EVAL_RO" | "EVALSHA_RO"
        | "FCALL" | "FCALL_RO" | "FUNCTION" | "SCRIPT" | "RESTORE" | "SAVE" | "BGSAVE"
        | "BGREWRITEAOF" => {
            return Ok((QueryRisk::Destructive, "redis_destructive_or_programmable"));
        }
        "SET" | "SETEX" | "PSETEX" | "SETNX" | "MSET" | "MSETNX" | "GETSET" | "GETDEL"
        | "GETEX" | "APPEND" | "SETRANGE" | "INCR" | "INCRBY" | "INCRBYFLOAT" | "DECR"
        | "DECRBY" | "DEL" | "UNLINK" | "RENAME" | "RENAMENX" | "EXPIRE" | "PEXPIRE"
        | "EXPIREAT" | "PEXPIREAT" | "PERSIST" | "COPY" | "MOVE" | "HSET" | "HSETNX" | "HMSET"
        | "HDEL" | "HINCRBY" | "HINCRBYFLOAT" | "HEXPIRE" | "HPEXPIRE" | "HPERSIST" | "LPUSH"
        | "RPUSH" | "LPUSHX" | "RPUSHX" | "LPOP" | "RPOP" | "LSET" | "LTRIM" | "LREM"
        | "LINSERT" | "RPOPLPUSH" | "LMOVE" | "LMPOP" | "SADD" | "SREM" | "SPOP" | "SMOVE"
        | "SUNIONSTORE" | "SINTERSTORE" | "SDIFFSTORE" | "ZADD" | "ZREM" | "ZINCRBY"
        | "ZPOPMIN" | "ZPOPMAX" | "ZMPOP" | "ZRANGESTORE" | "ZREMRANGEBYRANK"
        | "ZREMRANGEBYSCORE" | "ZREMRANGEBYLEX" | "ZUNIONSTORE" | "ZINTERSTORE" | "ZDIFFSTORE"
        | "XADD" | "XDEL" | "XTRIM" | "XACK" | "XCLAIM" | "XAUTOCLAIM" | "XGROUP"
        | "XREADGROUP" | "GEOADD" | "GEOSEARCHSTORE" | "BITOP" | "SETBIT" | "BITFIELD"
        | "PFADD" | "PFMERGE" | "SORT" | "GEORADIUS" | "GEORADIUSBYMEMBER" => {
            if count == 0 {
                return Err(DbError::InvalidRequest);
            }
            return Ok((QueryRisk::Write, "redis_write"));
        }
        _ => return Err(DbError::PolicyDenied),
    }
    Ok((QueryRisk::ReadOnly, "redis_read"))
}

fn bounded_integer(value: &[u8], minimum: u64, maximum: u64) -> Result<(), DbError> {
    let value = std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= minimum && *value <= maximum);
    if value.is_some() {
        Ok(())
    } else {
        Err(DbError::InvalidRequest)
    }
}

fn word(value: &[u8]) -> Result<String, DbError> {
    if value.is_empty()
        || !value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return Err(DbError::InvalidRequest);
    }
    Ok(String::from_utf8(value.to_ascii_uppercase()).map_err(|_| DbError::InvalidRequest)?)
}

pub(super) fn tokenize(text: &str) -> Result<Vec<Vec<Vec<u8>>>, DbError> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut argv = Vec::new();
    let mut token = Vec::new();
    let mut quoted = None;
    let mut active = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match (quoted, byte) {
            (Some(quote), value) if value == quote => {
                quoted = None;
                active = true;
            }
            (None, b'\'' | b'"') => {
                quoted = Some(byte);
                active = true;
            }
            (_, b'\\') => {
                index += 1;
                let next = *bytes.get(index).ok_or(DbError::InvalidRequest)?;
                match next {
                    b'n' => token.push(b'\n'),
                    b'r' => token.push(b'\r'),
                    b't' => token.push(b'\t'),
                    b'b' => token.push(8),
                    b'a' => token.push(7),
                    b'x' => {
                        let hex = bytes
                            .get(index + 1..index + 3)
                            .ok_or(DbError::InvalidRequest)?;
                        let hex = std::str::from_utf8(hex).map_err(|_| DbError::InvalidRequest)?;
                        token.push(
                            u8::from_str_radix(hex, 16).map_err(|_| DbError::InvalidRequest)?,
                        );
                        index += 2;
                    }
                    value => token.push(value),
                }
                active = true;
            }
            (None, b'#') if !active && argv.is_empty() => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            (None, b' ' | b'\t' | b'\r' | b'\n') => {
                if active {
                    argv.push(std::mem::take(&mut token));
                    active = false;
                }
                if byte == b'\n' && !argv.is_empty() {
                    lines.push(std::mem::take(&mut argv));
                }
            }
            (_, value) => {
                token.push(value);
                active = true;
            }
        }
        if token.len() > 65536 || argv.len() > 1024 || lines.len() > MAX_STATEMENTS {
            return Err(DbError::ResultLimit);
        }
        index += 1;
    }
    if quoted.is_some() {
        return Err(DbError::InvalidRequest);
    }
    if active {
        argv.push(token);
    }
    if !argv.is_empty() {
        lines.push(argv);
    }
    Ok(lines)
}
