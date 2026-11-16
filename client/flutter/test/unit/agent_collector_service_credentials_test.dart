import 'dart:convert';

import 'package:consolecrypt/core/bridge/rust_agent_collector_service.dart';
import 'package:consolecrypt/core/bridge/rust_agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_credentials.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_port.dart';
import 'package:consolecrypt/monitoring/agent_collector_service_view.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/agent_collector_service_credentials_fixture.dart' as c;
import '../support/agent_collector_service_fixture.dart' as v;
import '../support/agent_collector_service_flow_fixture.dart' as f;
import '../support/agent_collector_service_revocation_fixture.dart' as r;
import 'agent_collector_service_revocation_test.dart' as u;

void main() {
  test(
    'actual local choices use only original frame; exact saved refs and target revisions bound before approval',
    () async {
      final h = c.CredentialNative(),
          requests = <Map<String, dynamic>>[],
          choiceRequests = <Map<String, dynamic>>[],
          target = u.target();
      final previous = u.port(r.RevokeNative(target));
      final original = RustAgentCollectorServicePort(
        prepareOriginal: (raw) async {
          requests.add(jsonDecode(raw) as Map<String, dynamic>);
          return h;
        },
        readCatalogJson: (_) async => f.catalogReply(),
      );
      final port = RustAgentCollectorServiceWithCredentials(
        original: original,
        status: previous,
        revocation: previous,
        readJson: (raw) async {
          choiceRequests.add(jsonDecode(raw) as Map<String, dynamic>);
          return jsonEncode(c.choicesWire());
        },
      );
      expect(requests, isEmpty);
      expect(choiceRequests, isEmpty);
      final choices = await port.readCredentialChoices(f.scope), choice = choices.choices.single;
      expect(choiceRequests.single, {
        'version': 1,
        'profileId': f.scope.profileId,
        'notificationScopeId': f.scope.nonce,
      });
      final intent = AgentCollectorServiceIntent(
        f.record,
        f.catalog,
        credentials: [
          AgentCollectorServiceCredentialSelection(
            credentialId: choice.id,
            credentialRevision: choice.revision,
            targetId: f.catalog.manifest.targets.first.id,
          ),
          AgentCollectorServiceCredentialSelection(credentialId: choice.id, credentialRevision: choice.revision),
        ],
      );
      final handle = await port.prepare(f.scope, intent), view = await handle.details();
      expect(requests.single['credentials'], intent.credentials.map((r) => r.request()).toList());
      expect(view.credentialSelections.length, 2);
      expect(view.credentialSelections.last.credentialRevision, '7');
      expect(intent.matches(view), isTrue);
      handle.cancel();
      await handle.joined();
      handle.dispose();
    },
  );
  test('closed credential variant denies secret fields, wrong target/revision, duplicate slots and public/credential substitution', () {
    final valid = c.selectedWire(), source = (valid['credentialSelections'] as List).first as Map<String, dynamic>;
    final public = parseAgentCollectorServiceView(jsonEncode(v.serviceWire()), f.scope, f.record);
    expect(public.credentialSelections, isEmpty);
    expect(f.intent().request(f.scope).containsKey('credentials'), isFalse);
    for (final value in <Map<String, dynamic>>[
      {...valid, 'credentialSelections': <Object>[]},
      {
        ...valid,
        'credentialSelections': [source, source],
      },
      {
        ...valid,
        'credentialSelections': [
          {...source, 'password': 'owned forbidden text'},
        ],
      },
      {
        ...valid,
        'credentialSelections': [
          {...source, 'credentialRevision': 7},
        ],
      },
      {
        ...valid,
        'credentialSelections': [
          {
            ...source,
            'target': {'kind': 'scrape', 'targetId': 'missing'},
          },
        ],
      },
      {...valid, 'kind': 'finiteCollectorService'},
    ]) {
      expect(() => parseAgentCollectorServiceView(jsonEncode(value), f.scope, f.record), throwsFormatException);
    }
    final checked = parseAgentCollectorServiceView(jsonEncode(valid), f.scope, f.record);
    expect(f.intent().matches(checked), isFalse);
    expect(public.sameOriginal(checked), isFalse);
    final choices = c.choicesWire();
    for (final value in <Map<String, dynamic>>[
      {...choices, 'version': 1.0},
      {...choices, 'notificationScopeId': 'foreign'},
      {
        ...choices,
        'credentials': [
          {'credentialId': c.credentialId, 'credentialRevision': '7', 'name': 'Owned', 'username': 'forbidden'},
        ],
      },
    ]) {
      expect(() => parseAgentCollectorServiceCredentialChoices(jsonEncode(value), f.scope), throwsFormatException);
    }
    choices['credentials'] = [
      {'credentialId': c.credentialId, 'credentialRevision': '9223372036854775807', 'name': 'Owned'},
    ];
    expect(
      parseAgentCollectorServiceCredentialChoices(jsonEncode(choices), f.scope).choices.single.revision,
      '9223372036854775807',
    );
  });
}
