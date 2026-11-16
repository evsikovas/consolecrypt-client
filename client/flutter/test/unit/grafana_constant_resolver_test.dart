import 'dart:convert';

import 'package:consolecrypt/monitoring/grafana_constant_review.dart';
import 'package:flutter_test/flutter_test.dart' as ft;

void test(String name, void Function() body) => ft.test(name, body);
void expect(bool value) => ft.expect(value, ft.isTrue);

String source(String expression, {String variableType = 'constant', bool transform = false, bool duplicate = false}) =>
    jsonEncode({
      'schemaVersion': 42,
      'title': 'Original dashboard',
      'templating': {
        'list': [
          {
            'name': 'node',
            'type': variableType,
            'current': {'value': 'SAVED_PRIVATE_VALUE_MUST_NOT_BE_READ'},
          },
          if (duplicate) {'name': 'node', 'type': 'constant'},
        ],
      },
      'panels': [
        {
          'type': 'timeseries',
          'title': 'CPU',
          'datasource': {'type': 'prometheus', 'uid': 'manual-source'},
          'targets': [
            {'expr': expression},
          ],
          'transformations': transform
              ? [
                  {'id': 'reduce'},
                ]
              : <Map<String, String>>[],
        },
        {'type': 'unsupported-plugin', 'title': 'Preserve me', 'pluginSecret': 'SYNTHETIC_ORIGINAL_ONLY'},
      ],
    });
GrafanaConstantReview resolve(String expression, Map<String, String> values, {String type = 'constant'}) =>
    reviewGrafanaConstants(source(expression, variableType: type), 'panels/0', values);
void denied(
  String name,
  String expression,
  Map<String, String> values,
  GrafanaConstantProblem problem, {
  String type = 'constant',
}) {
  test(name, () {
    try {
      resolve(expression, values, type: type);
    } on GrafanaConstantException catch (e) {
      expect(e.problem == problem);
      return;
    }
    throw StateError('Expected explicit constant refusal');
  });
}

void main() {
  denied('comment-only apparent matcher is never resolved', r'up # {job="$node"}', {
    'node': 'prod',
  }, GrafanaConstantProblem.unsupportedContext);
  test('real matcher and ordinary comment braces remain correct', () {
    const expression = 'up{job="\$node"} # {unclosed " ordinary comment';
    final r = resolve(expression, {'node': 'prod'});
    expect(r.draft.expression == 'up{job="prod"} # {unclosed " ordinary comment');
  });
  test('comment between selector opening and real matcher is skipped lexically', () {
    const expression = 'up{ # { comment "ignored"\n job="\$node"}';
    expect(resolve(expression, {'node': 'prod'}).draft.expression == 'up{ # { comment "ignored"\n job="prod"}');
  });
  test('manual exact label constant, original all panels unchanged', () {
    final raw = source(r'up{instance="$node"}');
    final r = reviewGrafanaConstants(raw, 'panels/0', {'node': 'node-a:9100'});
    expect(r.draft.expression == 'up{instance="node-a:9100"}');
    expect(r.original.originalJson == raw && r.original.panels.length == 2 && r.names.single == 'node');
    expect(!r.draft.expression.contains('SAVED_PRIVATE_VALUE'));
  });
  test('regex metacharacters escaped as literal, never broadened', () {
    final r = resolve(r'up{instance=~"${node:regex}"}', {'node': 'node-a.example:9100|.*'});
    expect(r.draft.expression == 'up{instance=~${jsonEncode(RegExp.escape('node-a.example:9100|.*'))}}');
  });
  test('negated literal matching keeps operator', () {
    expect(resolve(r'up{instance!="$node"}', {'node': 'prod'}).draft.expression == 'up{instance!="prod"}');
  });
  test('regex negation escaped', () {
    expect(
      resolve(r'up{instance!~"$node"}', {'node': '.*'}).draft.expression ==
          'up{instance!~${jsonEncode(RegExp.escape('.*'))}}',
    );
  });
  test('quotes/backslashes cannot close matcher or inject PromQL', () {
    const value = 'a"} or up{job="all\\tail';
    expect(resolve(r'up{instance="$node"}', {'node': value}).draft.expression == 'up{instance=${jsonEncode(value)}}');
  });
  test('braced variable and legacy variable supported explicitly', () {
    expect(resolve(r'up{instance="${node}"}', {'node': 'p'}).draft.expression == 'up{instance="p"}');
    expect(resolve(r'up{instance="[[node]]"}', {'node': 'p'}).draft.expression == 'up{instance="p"}');
  });
  test('same variable repeated, one reviewed name', () {
    final r = resolve(r'up{job="$node"} + down{job="$node"}', {'node': 'p'});
    expect(r.names.length == 1 && r.draft.expression == 'up{job="p"} + down{job="p"}');
  });
  test('empty and Unicode values remain explicit', () {
    expect(resolve(r'up{job="$node"}', {'node': ''}).draft.expression == 'up{job=""}');
    expect(resolve(r'up{job="$node"}', {'node': 'сервер'}).draft.expression == 'up{job="сервер"}');
  });
  test('literal regex dollar anchor is not a substitution', () {
    expect(resolve(r'up{job=~"^node$"}', {}).draft.expression == r'up{job=~"^node$"}');
  });
  denied(
    'saved current value never supplies missing input',
    r'up{job="$node"}',
    {},
    GrafanaConstantProblem.missingValue,
  );
  denied(
    'query variable never fetched',
    r'up{job="$node"}',
    {'node': 'p'},
    GrafanaConstantProblem.unsupportedVariable,
    type: 'query',
  );
  denied(
    'datasource variable never mapped',
    r'up{job="$node"}',
    {'node': 'p'},
    GrafanaConstantProblem.unsupportedVariable,
    type: 'datasource',
  );
  denied(
    'custom multichoice not silently flattened',
    r'up{job="$node"}',
    {'node': 'p'},
    GrafanaConstantProblem.unsupportedVariable,
    type: 'custom',
  );
  denied('undeclared value rejected', r'up{job="$node"}', {'other': 'p'}, GrafanaConstantProblem.unsupportedVariable);
  denied('system macro not resolved', r'rate(up[$__rate_interval])', {}, GrafanaConstantProblem.unsupportedContext);
  denied('variable outside selector string unsupported', r'rate(up[$node])', {
    'node': '5m',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('compound interpolation unsupported', r'up{job="prefix-$node"}', {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('single quote interpolation unsupported', r"up{job='$node'}", {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('backtick interpolation unsupported', r'up{job=`$node`}', {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('function strings unsupported', r'label_replace(up,"x","$node","y",".*")', {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('regex format cannot become exact match', r'up{job="${node:regex}"}', {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  denied('control input rejected', r'up{job="$node"}', {'node': 'p\nq'}, GrafanaConstantProblem.invalidBindings);
  denied('reserved builtin value not impersonated', r'up{job="$node"}', {
    '__range': '5m',
  }, GrafanaConstantProblem.invalidBindings);
  denied('second variable token in value rejected explicitly', r'up{job="$node"}', {
    'node': r'$other',
  }, GrafanaConstantProblem.invalidBindings);
  denied('value byte budget', r'up{job="$node"}', {'node': 'x' * 513}, GrafanaConstantProblem.invalidBindings);
  denied('unterminated string is fixed refusal', r'up{job="$node', {
    'node': 'p',
  }, GrafanaConstantProblem.unsupportedContext);
  test('unsupported transformation preserved, not bypassed', () {
    try {
      reviewGrafanaConstants(source(r'up{job="$node"}', transform: true), 'panels/0', {'node': 'p'});
    } on GrafanaConstantException catch (e) {
      expect(e.problem == GrafanaConstantProblem.unsupportedPanel);
      return;
    }
    throw StateError('Transformation silently discarded');
  });
  test('duplicate declaration refused', () {
    try {
      reviewGrafanaConstants(source(r'up{job="$node"}', duplicate: true), 'panels/0', {'node': 'p'});
    } on GrafanaConstantException catch (e) {
      expect(e.problem == GrafanaConstantProblem.invalidBindings);
      return;
    }
    throw StateError('Ambiguous variable declaration accepted');
  });
  test('unknown panel path not interpreted as object path', () {
    try {
      reviewGrafanaConstants(source('up'), '../../secret', {});
    } on GrafanaConstantException catch (e) {
      expect(e.problem == GrafanaConstantProblem.unknownPanel);
      return;
    }
    throw StateError('Unknown panel accepted');
  });
  test('expanded query budget enforced', () {
    final expression = List.filled(60, r'up{job="$node"}').join('+');
    try {
      resolve(expression, {'node': 'x' * 512});
    } on GrafanaConstantException catch (e) {
      expect(e.problem == GrafanaConstantProblem.budget);
      return;
    }
    throw StateError('Expansion exceeded query budget');
  });
}
