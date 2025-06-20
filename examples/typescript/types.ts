// 导入语句
import { useState, useEffect } from 'react';
import axios from 'axios';
import * as _ from 'lodash';

// 接口定义
interface User {
    id: number;
    name: string;
    email: string;
    age?: number; // 可选属性
    readonly role: string; // 只读属性
}

// 枚举定义
enum TaskStatus {
    TODO = 'todo',
    IN_PROGRESS = 'in_progress',
    DONE = 'done',
}

// 类型别名
type UserID = string | number;
type Callback<T> = (data: T) => void;

// 类定义
class UserService {
    private apiUrl: string;

    constructor(baseUrl: string) {
        this.apiUrl = `${baseUrl}/users`;
    }

    // 实例方法
    public async getUser(userID: UserID): Promise<User[]> {
        try {
            const response = await axios.get<User[]>(this.apiUrl);
            return response.data;
        } catch (error) {
            console.error('Failed to fetch users:', error);
            return [];
        }
    }

    // 带泛型的方法
    public static filterUsers<T extends User>(users: T[], predicate: (user: T) => boolean): T[] {
        return users.filter(predicate);
    }
}

export {
    User,
    TaskStatus,
    UserID,
    Callback,
    UserService,
}