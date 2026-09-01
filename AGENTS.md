# 项目设计规范
- 项目处于敏捷开发阶段，重构时不考虑后向兼容，不保留历史代码

# agent 角色定义
- 开启新会话后向 /tmp/${repo_name}/agents.jsonl 注册会话的session_id,title,role.
- 初始 `role = null`, 用户明确职责和可以更新title,role.
- 需要向其它会话通信时请参考 /tmp/${repo_name}/agents.jsonl.