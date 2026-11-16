"""Bounded diagnostic projection, never execution/admission evidence.

Known secret masking remains a separate parent-side step. This projection does
not claim to discover arbitrary secrets printed by user-controlled tasks.
"""


def diagnostic_result(value):
    nodes = 1024
    characters = 64 * 1024

    def visit(item, depth):
        nonlocal nodes, characters
        nodes -= 1
        if nodes < 0 or depth > 12:
            return {'truncated': True}
        if isinstance(item, dict):
            # Apply at every loop/result nesting level, before copying fields.
            if item.get('_ansible_no_log'):
                return {'censored': True}
            result = {}
            omitted = []
            for key, child in item.items():
                if not isinstance(key, str):
                    omitted.append('unsupported_key')
                    continue
                if key in ('ansible_facts', 'invocation'):
                    omitted.append(key)
                    continue
                if len(result) >= 256 or nodes <= 0 or len(key) > 256:
                    omitted.append('budget')
                    break
                result[key] = visit(child, depth + 1)
            if omitted:
                result['_consolecrypt_omitted'] = sorted(set(omitted))
            return result
        if isinstance(item, list):
            result = []
            for child in item:
                if len(result) >= 256 or nodes <= 0:
                    result.append({'truncated': True})
                    break
                result.append(visit(child, depth + 1))
            return result
        if isinstance(item, str):
            limit = min(16 * 1024, characters)
            if limit <= 0:
                return '[TRUNCATED]'
            text = item[:limit]
            characters -= len(text)
            return text + (' [TRUNCATED]' if len(item) > limit else '')
        if item is None or type(item) in (bool, int, float):
            return item
        return {'truncated': True}

    return visit(value, 0)
