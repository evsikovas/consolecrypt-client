import 'dart:convert';

import 'package:consolecrypt/core/bridge/mapping.dart';
import 'package:consolecrypt/db/db_designer_models.dart';
import 'package:consolecrypt/db/db_models.dart';
import 'package:consolecrypt/src/rust/api/database_designer.dart' as native;

/// The table designer of the native core: dialects, DDL plans, the current
/// structure of a table and applying a reviewed designer script.
mixin RustDbTableDesign implements DbTableDesignService {
  final _dialects = <DbEngine, DbDesignDialect>{};

  @override
  Future<DbDesignDialect> designDialect(DbEngine engine) async => _dialects[engine] ??= DbDesignDialect.fromJson(
    jsonDecode(await guard(() => native.dbDesignDialect(engineName: engine.wireName))) as DbJson,
  );

  @override
  Future<DbDesignPlan> planDesign(DbEngine engine, DbTableDesign? original, DbTableDesign design) async =>
      DbDesignPlan.fromJson(
        jsonDecode(
          await guard(
            () => native.dbDesignPlan(
              engineName: engine.wireName,
              originalJson: original?.encode(),
              designJson: design.encode(),
            ),
          ),
        ) as DbJson,
      );

  @override
  Future<DbTableDesign> loadTableDesign(String sessionId, DbObjectRef table) async => DbTableDesign.fromJson(
    jsonDecode(await guard(() => native.dbTableDesign(sessionId: sessionId, tableJson: jsonEncode(table.toJson()))))
        as DbJson,
  );

  @override
  Future<DbDesignApplyResult> applyDesign(String sessionId, DbQueryRequest request) async =>
      DbDesignApplyResult.fromJson(
        jsonDecode(
          await guard(() => native.dbApplyDesign(sessionId: sessionId, requestJson: jsonEncode(request.toJson()))),
        ) as DbJson,
      );
}
