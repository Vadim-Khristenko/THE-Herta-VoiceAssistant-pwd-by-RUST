from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from typing import Any, Literal


ToolParameterType = Literal['string', 'boolean', 'integer', 'number']


@dataclass(frozen=True, slots=True)
class ToolParameter:
    name: str
    parameter_type: ToolParameterType
    description: str
    required: bool = True


@dataclass(frozen=True, slots=True)
class ToolSpec:
    name: str
    description: str
    parameters: tuple[ToolParameter, ...] = ()
    destructive: bool = False


@dataclass(frozen=True, slots=True)
class ToolCall:
    name: str
    arguments: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True)
class ToolResult:
    action_name: str
    message: str
    executed: bool
    data: dict[str, Any] = field(default_factory=dict)

    def to_function_response(self) -> dict[str, Any]:
        response = {
            'action_name': self.action_name,
            'message': self.message,
            'executed': self.executed,
        }
        if self.data:
            response['data'] = self.data
        return response


class CallableTool:
    def __init__(self, spec: ToolSpec, handler: Callable[[ToolCall], ToolResult]) -> None:
        self.spec = spec
        self._handler = handler

    def run(self, call: ToolCall) -> ToolResult:
        missing_parameters = [
            parameter.name
            for parameter in self.spec.parameters
            if parameter.required and parameter.name not in call.arguments
        ]
        if missing_parameters:
            missing = ', '.join(missing_parameters)
            raise ValueError(f"Tool '{self.spec.name}' is missing required parameter(s): {missing}")
        return self._handler(call)


class ToolRegistry:
    def __init__(self, tools: list[CallableTool]) -> None:
        self._tools = {tool.spec.name: tool for tool in tools}

    @property
    def specs(self) -> list[ToolSpec]:
        return [tool.spec for tool in self._tools.values()]

    def run(self, call: ToolCall) -> ToolResult:
        tool = self._tools.get(call.name)
        if tool is None:
            raise ValueError(f'Unknown tool: {call.name}')
        if tool.spec.destructive:
            return ToolResult(
                action_name=tool.spec.name,
                message=f"Tool '{tool.spec.name}' is marked destructive and cannot run.",
                executed=False,
            )
        return tool.run(call)


def build_function_declarations(tool_specs: list[ToolSpec]) -> list[dict[str, Any]]:
    declarations: list[dict[str, Any]] = []
    for spec in tool_specs:
        declaration: dict[str, Any] = {
            'name': spec.name,
            'description': spec.description,
        }
        if spec.parameters:
            properties: dict[str, dict[str, str]] = {}
            required: list[str] = []
            for parameter in spec.parameters:
                properties[parameter.name] = {
                    'type': parameter.parameter_type,
                    'description': parameter.description,
                }
                if parameter.required:
                    required.append(parameter.name)

            parameters_schema: dict[str, Any] = {
                'type': 'object',
                'properties': properties,
            }
            if required:
                parameters_schema['required'] = required
            declaration['parameters'] = parameters_schema
        declarations.append(declaration)
    return declarations
