# Nacos OpenAPI 接口文档 📘

> 本文已“重新读取”并对齐仓库根目录的 `nacos-openapi.txt`，整理为可检索、可复制的接口速查。所有文案与示例均为中文并配上贴切 emoji，便于联调与排查。✅😊

## 基础说明
- 基础地址：如 `http://127.0.0.1:8848` 或你的集群地址 🌐
- 鉴权方式：登录获取 `accessToken`，后续调用在查询参数中携带 `accessToken=...` 🔐
- 内容编码：建议 UTF-8（避免 MD5/差异比较异常）🧩
- 统一返回体：所有接口响应为 JSON，格式如下 📦

```json
{
  "code": 0,
  "message": "success",
  "data": {}
}
```

- 字段含义：
  - `code`：整型错误码，0 表示成功，非 0 表示失败 ⚠️
  - `message`：提示信息，成功为 `success` 💬
  - `data`：任意类型的数据载荷；失败时通常为详细出错信息 📄

## 错误码汇总 🧾
- `0`：success（成功）✅
- `10000`：parameter missing（参数缺失）⚠️
- `10001`：access denied（访问拒绝）⛔
- `10002`：data access error（数据访问错误）🐛
- `20001`：'tenant' parameter error（tenant 参数错误）
- `20002`：parameter validate error（参数验证错误）
- `20003`：MediaType Error（请求的 MediaType 错误）
- `20004`：resource not found（资源未找到）
- `20005`：resource conflict（资源冲突）
- `20006`：config listener is null（监听配置为空）
- `20007`：config listener error（监听配置错误）
- `20008`：invalid dataId（无效 dataId/鉴权失败）
- `20009`：parameter mismatch（请求参数不匹配）
- `21000`：service name error（服务名错误）
- `21001`：weight error（权重参数错误）
- `21002`：instance metadata error（实例 metadata 错误）
- `21003`：instance not found（实例不存在）
- `21004`：instance error（实例信息错误）
- `21005`：service metadata error（服务 metadata 错误）
- `21006`：selector error（访问策略错误）
- `21007`：service already exist（服务已存在）
- `21008`：service not exist（服务不存在）
- `21009`：service delete failure（存在实例，删除失败）
- `21010`：healthy param miss（healthy 参数缺失）
- `21011`：health check still running（健康检查运行中）
- `22000`：illegal namespace（命名空间不合法）
- `22001`：namespace not exist（命名空间不存在）
- `22002`：namespace already exist（命名空间已存在）
- `23000`：illegal state（状态不合法）
- `23001`：node info error（节点信息错误）
- `23002`：node down failure（节点离线失败）
- `30000`：server error（其他内部错误）

---

## 接口目录 🧭
- 登录获取 Token（v1）：`POST /nacos/v1/auth/login` 🔐
- 获取配置：`GET /nacos/v2/cs/config` 📄
- 发布配置（新增/覆盖）：`POST /nacos/v2/cs/config` 📝
- 查询配置列表：`GET /nacos/v2/cs/history/configs` 📚
- 查询系统指标：`GET /nacos/v2/ns/operator/metrics` 📊
- 查询命名空间详情：`GET /nacos/v2/console/namespace` 🔎
- 创建命名空间：`POST /nacos/v2/console/namespace` ➕
- 编辑命名空间：`PUT /nacos/v2/console/namespace` ✏️
- 删除命名空间：`DELETE /nacos/v2/console/namespace` 🗑️
- 查询命名空间列表：`GET /nacos/v2/console/namespace/list` 📃

---

## 登录获取 Token（v1）🔐
- 方法：`POST`
- 路径：`/nacos/v1/auth/login`
- 表单参数：
  - `username`：登录名，如 `nacos` 👤
  - `password`：密码，如 `nacos` 🔑
- 响应示例：
```json
{"accessToken":"<token>","tokenTtl":18000,"globalAdmin":true,"username":"nacos"}
```
- cURL：
```bash
curl -sS -X POST "http://127.0.0.1:8848/nacos/v1/auth/login" \
  -d "username=nacos" \
  -d "password=nacos"
```

## 获取配置（v2）📄
- 方法：`GET`
- 路径：`/nacos/v2/cs/config`
- 查询参数：
  - `namespaceId`：命名空间（默认 `public`）🧭
  - `group`：配置分组，如 `DEFAULT_GROUP` 🗂️
  - `dataId`：配置名，如 `nacos.example` 🏷️
  - `tag`：可选，标签 🏷️
- 返回数据：`data` 为配置内容字符串 🧾
- cURL：
```bash
curl -sS -X GET "http://127.0.0.1:8848/nacos/v2/cs/config?dataId=nacos.example&group=DEFAULT_GROUP&namespaceId=public"
```

## 发布配置（v2）📝
- 方法：`POST`
- 路径：`/nacos/v2/cs/config`
- 查询参数：
  - `accessToken`：可选，若开启鉴权需携带 🔐
- 表单参数：
  - `dataId`：配置唯一标识（如 `nacos.example`）🏷️
  - `group`：配置分组（如 `DEFAULT_GROUP`）🗂️
  - `namespaceId`：命名空间（如 `public`）🧭
  - `content`：配置内容（原文字符串）📝
  - `type`：可选，内容类型（`yaml`/`json`/`properties`/`text`）
- 返回数据：`data` 为 `true/false` 表示是否成功 ✅/❌
- cURL：
```bash
curl -sS -X POST "http://127.0.0.1:8848/nacos/v2/cs/config?accessToken=<token>" \
  -d "dataId=nacos.example" \
  -d "group=DEFAULT_GROUP" \
  -d "namespaceId=public" \
  -d "content=contentTest" \
  -d "type=yaml"
```

## 查询配置列表（v2）📚
- 方法：`GET`
- 路径：`/nacos/v2/cs/history/configs`
- 查询参数：
  - `namespaceId`：必填，命名空间 ID 🧭
- 说明：响应中仅 `dataId`、`group`、`tenant`、`appName`、`type` 字段有效，其余为默认值 ℹ️
- cURL：
```bash
curl -sS "http://127.0.0.1:8848/nacos/v2/cs/history/configs?namespaceId=<nsId>"
```
- 返回示例（节选）：
```json
{
  "code": 0,
  "message": "success",
  "data": [
    {
      "id": "0",
      "dataId": "nacos.example",
      "group": "com.alibaba.nacos",
      "content": null,
      "md5": null,
      "encryptedDataKey": null,
      "tenant": "",
      "appName": "",
      "type": "yaml",
      "lastModified": 0
    }
  ]
}
```

## 查询系统当前数据指标（v2）📊
- 方法：`GET`
- 路径：`/nacos/v2/ns/operator/metrics`
- 查询参数：
  - `onlyStatus`：布尔，可选；仅显示状态（默认 `true`）⚙️
- 返回数据：`data` 为系统指标对象，含 `status/serviceCount/instanceCount/subscribeCount/...` 等字段 📈
- cURL：
```bash
curl -sS -X GET "http://127.0.0.1:8848/nacos/v2/ns/operator/metrics"
```
- 返回示例（节选）：
```json
{
  "code": 0,
  "message": "success",
  "data": {
    "status": "UP",
    "serviceCount": 2,
    "instanceCount": 2,
    "subscribeCount": 2,
    "raftNotifyTaskCount": 0,
    "clientCount": 2,
    "cpu": 0,
    "load": -1,
    "mem": 1
  }
}
```

## 查询具体命名空间（v2 Console）🔎
- 方法：`GET`
- 路径：`/nacos/v2/console/namespace`
- 查询参数：
  - `namespaceId`：命名空间 ID 🆔
- 返回数据：
  - `namespace`、`namespaceShowName`、`namespaceDesc`、`quota`、`configCount`、`type` ℹ️（`type`：0 全局 / 1 默认私有 / 2 自定义）
- cURL：
```bash
curl -sS -X GET "http://127.0.0.1:8848/nacos/v2/console/namespace?namespaceId=test_namespace"
```

## 创建命名空间（v2 Console）➕
- 方法：`POST`
- 路径：`/nacos/v2/console/namespace`
- 表单参数：
  - `namespaceId`：必填 🆔
  - `namespaceName`：必填 🏷️
  - `namespaceDesc`：可选 📝
- 返回数据：`data` 为 `true/false` ✅/❌
- cURL：
```bash
curl -sS -X POST "http://127.0.0.1:8848/nacos/v2/console/namespace" \
  -d "namespaceId=test_namespace" \
  -d "namespaceName=test" \
  -H "Content-Type: application/x-www-form-urlencoded"
```

## 编辑命名空间（v2 Console）✏️
- 方法：`PUT`
- 路径：`/nacos/v2/console/namespace`
- 表单参数：
  - `namespaceId`：必填 🆔
  - `namespaceName`：必填 🏷️
  - `namespaceDesc`：可选 📝
- 返回数据：`data` 为 `true/false` ✅/❌
- cURL：
```bash
curl -sS -X PUT "http://127.0.0.1:8848/nacos/v2/console/namespace" \
  -d "namespaceId=test_namespace" \
  -d "namespaceName=test.nacos"
```

## 删除命名空间（v2 Console）🗑️
- 方法：`DELETE`
- 路径：`/nacos/v2/console/namespace`
- 查询参数：
  - `namespaceId`：必填 🆔
- 返回数据：`data` 为 `true/false` ✅/❌
- cURL：
```bash
curl -sS -X DELETE "http://127.0.0.1:8848/nacos/v2/console/namespace?namespaceId=test_namespace"
```

## 查询命名空间列表（v2 Console）📃
- 方法：`GET`
- 路径：`/nacos/v2/console/namespace/list`
- 查询参数：
  - `accessToken`：可选，若服务开启鉴权 🔐
- 返回数据：`data` 为数组，含命名空间对象字段 `namespace/namespaceShowName/namespaceDesc/quota/configCount/type` 📚
- cURL：
```bash
curl -sS "http://127.0.0.1:8848/nacos/v2/console/namespace/list"
# 若开启鉴权：
# curl -sS "http://127.0.0.1:8848/nacos/v2/console/namespace/list?accessToken=<token>"
```

---

## 常见提示与排障 🧯
- 401/403：检查 `accessToken` 是否传入/是否过期，必要时重新登录 🔁
- 404：确认路径版本是否一致（登录用 v1，配置用 v2）🛣️
- 发布失败但 200：请检查 `code/message/data`；`data=true` 代表成功 ✅
- 内容异常：确保 `type` 与实际内容匹配（如 YAML/JSON）🧪

---
本页内容来源于 `nacos-openapi.txt` 的最新整理。如需纳入更多条目，请更新源文件后告知，我会再次同步并简明呈现。🪄📄

